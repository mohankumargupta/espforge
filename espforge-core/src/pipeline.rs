//! The front half shared by `check`, `build` and `gen`: parse -> `# espforge`
//! core -> options -> flag overrides -> resolve -> `$` dependency graph.
//! One function, so the commands cannot disagree.
//!
//! Source discovery lives in [`crate::source`]; hardware lowering lives in
//! the `espforge-hardware` crate. Nothing in this module prints: the binary
//! decides how a failure looks. This module keeps validation
//! (`Checked`/`validate`) plus one lowering entry point, so `check.rs` and
//! `build.rs` cannot disagree about what is buildable.

use espf::context::Refs;
use espf::diag::{EspfError, node_span};
use espforge_bindings::drivers::generated_registry::route_driver;
use espforge_diagnostics::{Diagnostic, Diagnostics, codes};
use espforge_model::Meta;
use espforge_options::{Answers, ResolvedOptions};

use crate::answers::Overrides;
use crate::frontend::{self, Frontend};

// ---------------------------------------------------------------- validation

/// Why a pipeline stage stopped.
#[derive(Debug)]
pub enum Invalid {
    /// Located diagnostics (rendered as snippets, codes in JSON).
    Diagnostics(Diagnostics),
    /// A `<device>` with no compiled driver. The human view has always been
    /// the message alone on a `❌` line; JSON carries the located diagnostic
    /// (stable code + span) it has always carried. Both forms are decided
    /// here, rendered by the caller.
    Unrouted { diagnostic: Diagnostic },
    /// A command-line answer that has no source location. Kept as a CLI-level
    /// error so the human output stays the one-line `❌ ...` it has always
    /// been; JSON still gives it a diagnostic entry.
    Message(String),
}

impl From<Vec<EspfError>> for Invalid {
    fn from(errors: Vec<EspfError>) -> Self {
        Invalid::Diagnostics(errors.iter().map(EspfError::to_diagnostic).collect())
    }
}

impl From<Vec<Diagnostic>> for Invalid {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Invalid::Diagnostics(diagnostics)
    }
}

impl From<String> for Invalid {
    fn from(m: String) -> Self {
        Invalid::Message(m)
    }
}

impl Invalid {
    /// The diagnostics this failure carries, for `--format json`.
    pub fn diagnostics(&self) -> Diagnostics {
        match self {
            Invalid::Diagnostics(diagnostics) => diagnostics.clone(),
            Invalid::Unrouted { diagnostic } => vec![diagnostic.clone()],
            Invalid::Message(m) => vec![Diagnostic::error(
                codes::for_cli_message(m),
                0..0,
                m.clone(),
                "",
            )],
        }
    }

    /// The one-line `❌` text for a failure that has never had a snippet.
    pub fn message(&self) -> Option<&str> {
        match self {
            Invalid::Message(m) => Some(m),
            Invalid::Unrouted { diagnostic } => Some(&diagnostic.message),
            Invalid::Diagnostics(_) => None,
        }
    }
}

/// A fully validated spec: final settings (file + flag overrides), resolved
/// options and a checked `$` graph.
pub struct Checked<'a> {
    pub fe: Frontend<'a>,
    /// Core settings after flag overrides.
    pub meta: Meta,
    /// Option answers after flag overrides.
    pub answers: Answers,
    pub resolved: ResolvedOptions,
    /// `note: ...` lines for flags that changed something.
    pub notes: Vec<String>,
}

pub fn validate<'a>(src: &'a str, overrides: &Overrides) -> Result<Checked<'a>, Invalid> {
    let fe = frontend::load(src)?;

    // Overrides win over the file; collect a note when they change something.
    let mut meta = fe.meta.clone();
    let mut answers = fe.answers.clone();
    let mut notes = Vec::new();
    overrides.apply_core(&mut meta, &mut notes)?;
    overrides.apply_options(
        espforge_options::embedded_schema(),
        &mut answers,
        &mut notes,
    )?;

    let resolved = espforge_options::resolve(&meta, &answers, &fe.derived)?;

    // `$` dependency graph (all five sections exist after `load`).
    Refs::resolve(&fe.parsed.sections)?;

    Ok(Checked {
        fe,
        meta,
        answers,
        resolved,
        notes,
    })
}

/// The board `build` can generate code for today.
///
/// The one act `check` and `build` share: route every device to a compiled
/// driver, then `board::build` constructs the typed `Board` (claiming pins as
/// it goes), so a spec that lowers is buildable and there is no separate
/// "valid but unbuildable" state. Routing runs first so an unknown device
/// reports the driver problem, not the temporary "devices are not lowered"
/// refusal. Problems are diagnostics and render as snippets.
pub fn board(c: &Checked) -> Result<espforge_model::Board, Invalid> {
    route_devices(&c.fe.parsed.sections)?;
    espforge_hardware::board::build(&c.fe.parsed.sections, &c.meta).map_err(Invalid::from)
}

/// Every `<device>` tag must have a compile-time driver engine.
fn route_devices(
    sections: &std::collections::HashMap<String, Vec<espf::context::ParsedNode>>,
) -> Result<(), Invalid> {
    for device in sections.get("devices").map(Vec::as_slice).unwrap_or(&[]) {
        if let Err(message) = route_driver(device.tag_name, device) {
            return Err(Invalid::Unrouted {
                diagnostic: Diagnostic::error(
                    codes::DRIVER_UNKNOWN,
                    node_span(device),
                    message,
                    format!("no driver is compiled for <{}>", device.tag_name),
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_BUS: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    fn hello() -> &'static str {
        &espforge_examples::find("01.Basics/helloworld")
            .unwrap()
            .content
    }

    #[test]
    fn valid_spec_passes_and_lowers() {
        let c = validate(hello(), &Overrides::default())
            .ok()
            .expect("hello validates");
        assert!(c.notes.is_empty());
        let board = board(&c).unwrap();
        assert!(board.components.is_empty());
        assert!(board.services.is_empty());
    }

    #[test]
    fn flags_override_and_leave_a_note() {
        let overrides = Overrides {
            chip: Some("esp32s3".into()),
            ..Default::default()
        };
        let c = validate(hello(), &overrides).ok().expect("validates");
        assert_eq!(c.meta.chip, "esp32s3");
        assert_eq!(c.fe.meta.chip, "esp32c3", "the file's own value is kept");
        assert_eq!(c.notes.len(), 1);
    }

    #[test]
    fn bad_flags_are_messages_not_spans() {
        let overrides = Overrides {
            chip: Some("esp99".into()),
            ..Default::default()
        };
        assert!(matches!(
            validate(hello(), &overrides).err(),
            Some(Invalid::Message(_))
        ));
    }

    #[test]
    fn graph_errors_are_reported_with_spans() {
        let src = WITH_BUS.replace("$i2c0", "$nope");
        match validate(&src, &Overrides::default()).err() {
            Some(Invalid::Diagnostics(diagnostics)) => {
                assert_eq!(diagnostics[0].message, "unknown peripheral");
                assert_eq!(diagnostics[0].code, codes::GRAPH_UNKNOWN_PERIPHERAL);
                assert_eq!(diagnostics[0].primary, 182..187);
            }
            _ => panic!("expected a located error"),
        }
    }

    #[test]
    fn unsupported_hardware_is_a_failure_not_a_warning() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <pwm id=\"pwm0\" />\n# components\n# devices\n# services\n";
        let c = validate(src, &Overrides::default())
            .ok()
            .expect("graph is fine");
        let err = board(&c).unwrap_err();
        match err {
            Invalid::Diagnostics(diagnostics) => {
                assert!(
                    diagnostics
                        .iter()
                        .any(|e| e.labels.iter().any(|label| label.text.contains("<pwm>"))),
                    "{diagnostics:?}"
                );
                assert_eq!(diagnostics[0].code, codes::PERIPHERAL_UNSUPPORTED);
            }
            Invalid::Message(m) => panic!("expected spans, got message: {m}"),
            Invalid::Unrouted { diagnostic } => {
                panic!("expected lowering spans, got driver routing: {diagnostic:?}")
            }
        }
    }

    #[test]
    fn a_device_without_a_driver_is_reported_before_lowering() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"g\" pin=\"18\" />\n# components\n  <led id=\"red_led\" gpio=\"$g\" />\n# devices\n  <made_up_part id=\"d\" gpio_component=\"$red_led\" />\n# services\n";
        let c = validate(src, &Overrides::default())
            .ok()
            .expect("graph is fine");
        match board(&c).unwrap_err() {
            Invalid::Unrouted { diagnostic } => {
                assert_eq!(diagnostic.code, codes::DRIVER_UNKNOWN);
                assert!(
                    diagnostic.message.contains("made_up_part"),
                    "{diagnostic:?}"
                );
                assert_eq!(
                    diagnostic.labels[0].text,
                    "no driver is compiled for <made_up_part>"
                );
            }
            other => panic!("expected an unrouted device, got {other:?}"),
        }
    }
}

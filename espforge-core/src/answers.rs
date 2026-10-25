//! Command-line overrides, as values rather than as clap arguments.
//!
//! The binary owns the flags (`AnswerArgs`, with its `--option NAME=VALUE`
//! syntax and value-enum spellings); this crate owns what an override *means*:
//! which values win, when a `note:` is owed, and what counts as an unknown
//! chip or a malformed option. That split is why the library has no `clap`
//! dependency — a second front-end (an LSP, a test) can fill [`Overrides`] in
//! directly.

use espforge_model::{Console, Meta, Runtime};
use espforge_options::{Answers, OptValue, Origin, Schema, chips};

/// What a user asked for on the command line. `None` means "whatever the file
/// says"; an empty `options` list means no `--option` was given.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub name: Option<String>,
    pub chip: Option<String>,
    pub runtime: Option<Runtime>,
    pub console: Option<Console>,
    pub defaults: Option<bool>,
    /// Raw `NAME=VALUE` strings, in the order the user wrote them.
    pub options: Vec<String>,
}

fn runtime_name(r: Runtime) -> &'static str {
    match r {
        Runtime::Blocking => "blocking",
        Runtime::Embassy => "embassy",
    }
}

fn console_name(c: Console) -> &'static str {
    match c {
        Console::Uart => "uart",
        Console::JtagSerial => "jtag-serial",
    }
}

/// `<options name="value" />` as the user would have written it.
fn show_option(name: &str, v: &OptValue) -> String {
    match v {
        OptValue::Bool(b) => format!("<options {name}=\"{b}\" />"),
        OptValue::Str(s) => format!("<options {name}=\"{s}\" />"),
        OptValue::List(l) => {
            let items: Vec<String> = l.iter().map(|i| format!("{name}+=\"{i}\"")).collect();
            format!("<options {} />", items.join(" "))
        }
    }
}

impl Overrides {
    /// Override the core settings. Pushes a `note:` per value that changed.
    pub fn apply_core(&self, meta: &mut Meta, notes: &mut Vec<String>) -> Result<(), String> {
        if let Some(n) = &self.name
            && *n != meta.name
        {
            notes.push(format!(
                "note: --name {n} overrides <project name=\"{}\" />",
                meta.name
            ));
            meta.name = n.clone();
        }
        if let Some(c) = &self.chip {
            if !chips::is_supported_chip(c) {
                return Err(format!(
                    "unknown chip \"{c}\" for --chip (expected one of: {})",
                    chips::supported_chips().join(", ")
                ));
            }
            if *c != meta.chip {
                notes.push(format!(
                    "note: --chip {c} overrides <chip type=\"{}\" />",
                    meta.chip
                ));
                meta.chip = c.clone();
            }
        }
        if let Some(r) = self.runtime
            && r != meta.runtime
        {
            notes.push(format!(
                "note: --runtime {} overrides <runtime type=\"{}\" />",
                runtime_name(r),
                runtime_name(meta.runtime)
            ));
            meta.runtime = r;
        }
        if let Some(c) = self.console
            && c != meta.console
        {
            notes.push(format!(
                "note: --console {} overrides <console type=\"{}\" />",
                console_name(c),
                console_name(meta.console)
            ));
            meta.console = c;
        }
        Ok(())
    }

    /// Apply `--defaults` and `--option NAME=VALUE`. Pushes a `note:` when an
    /// option written in the `.espf` is changed to a different value.
    pub fn apply_options(
        &self,
        schema: &Schema,
        answers: &mut Answers,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        if let Some(d) = self.defaults {
            answers.defaults = d;
        }
        for raw in &self.options {
            let (name, value) = raw
                .split_once('=')
                .ok_or_else(|| format!("--option expects NAME=VALUE, got \"{raw}\""))?;
            if let Some(prev) = answers.apply_flag(schema, name, value)? {
                if matches!(prev.origin, Origin::Espf(_))
                    && prev.value != answers.values[name].value
                {
                    notes.push(format!(
                        "note: --option {name}={value} overrides {}",
                        show_option(name, &prev.value)
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_options::embedded_schema;

    fn meta() -> Meta {
        Meta {
            name: "hello".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        }
    }

    #[test]
    fn chip_flag_overrides_with_a_note() {
        let overrides = Overrides {
            chip: Some("esp32s3".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        overrides.apply_core(&mut m, &mut notes).unwrap();
        assert_eq!(m.chip, "esp32s3");
        assert_eq!(
            notes,
            ["note: --chip esp32s3 overrides <chip type=\"esp32c3\" />"]
        );
    }

    #[test]
    fn a_flag_equal_to_the_file_is_silent() {
        let overrides = Overrides {
            chip: Some("esp32c3".into()),
            runtime: Some(Runtime::Blocking),
            console: Some(Console::Uart),
            name: Some("hello".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        overrides.apply_core(&mut m, &mut notes).unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn runtime_console_and_name_notes() {
        let overrides = Overrides {
            runtime: Some(Runtime::Embassy),
            console: Some(Console::JtagSerial),
            name: Some("other".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        overrides.apply_core(&mut m, &mut notes).unwrap();
        assert_eq!(m.runtime, Runtime::Embassy);
        assert_eq!(m.console, Console::JtagSerial);
        assert!(notes.contains(
            &"note: --runtime embassy overrides <runtime type=\"blocking\" />".to_string()
        ));
        assert!(notes.contains(
            &"note: --console jtag-serial overrides <console type=\"uart\" />".to_string()
        ));
        assert!(
            notes.contains(&"note: --name other overrides <project name=\"hello\" />".to_string())
        );
    }

    #[test]
    fn an_unknown_chip_is_refused() {
        let overrides = Overrides {
            chip: Some("esp99".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        let err = overrides.apply_core(&mut m, &mut notes).unwrap_err();
        assert!(err.contains("unknown chip"), "{err}");
    }

    #[test]
    fn option_flag_overrides_the_espf_with_a_note() {
        let schema = embedded_schema();
        let mut answers = Answers::empty(0..0);
        answers.apply_flag(schema, "alloc", "true").unwrap();
        // pretend that came from the file
        answers.values.get_mut("alloc").unwrap().origin = Origin::Espf(0..1);
        let overrides = Overrides {
            options: vec!["alloc=false".into()],
            ..Default::default()
        };
        let mut notes = vec![];
        overrides
            .apply_options(schema, &mut answers, &mut notes)
            .unwrap();
        assert_eq!(answers.values["alloc"].value, OptValue::Bool(false));
        assert_eq!(
            notes,
            ["note: --option alloc=false overrides <options alloc=\"true\" />"]
        );
    }

    #[test]
    fn bad_option_flags_are_errors() {
        let schema = embedded_schema();
        let mut a = Answers::empty(0..0);
        let bad = |s: &str| Overrides {
            options: vec![s.into()],
            ..Default::default()
        };
        assert!(
            bad("alloc")
                .apply_options(schema, &mut a, &mut vec![])
                .is_err()
        );
        assert!(
            bad("nope=1")
                .apply_options(schema, &mut a, &mut vec![])
                .is_err()
        );
        assert!(
            bad("alloc=maybe")
                .apply_options(schema, &mut a, &mut vec![])
                .is_err()
        );
    }

    #[test]
    fn defaults_flag_is_applied() {
        let mut a = Answers::empty(0..0);
        let overrides = Overrides {
            defaults: Some(false),
            ..Default::default()
        };
        overrides
            .apply_options(embedded_schema(), &mut a, &mut vec![])
            .unwrap();
        assert!(!a.defaults);
    }
}

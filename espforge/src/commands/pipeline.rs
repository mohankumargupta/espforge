//! The front half shared by `check`, `build` and `gen`: find the source, then
//! parse -> `# espforge` core -> options -> flag overrides -> resolve -> `$`
//! dependency graph. One function, so the commands cannot disagree.

use espf::context::RuntimeContext;
use espf::diag::EspfError;
use espforge_emit::{Hardware, LedSpec};
use espforge_model::Meta;
use espforge_options::{Answers, ResolvedOptions};
use std::path::{Path, PathBuf};

use super::answers::AnswerArgs;
use super::frontend::{self, Frontend};

// -------------------------------------------------------------------- source

pub struct Source {
    /// Name used in diagnostics: the path, or `embedded:<category>/<name>`.
    pub name: String,
    pub text: String,
    /// Directory of the `.espf` (None for embedded examples).
    pub dir: Option<PathBuf>,
    pub example: Option<&'static espforge_examples::Example>,
}

fn from_file(file: &Path) -> Result<Source, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read '{}': {e}", file.display()))?;
    let dir = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Source {
        name: file.to_string_lossy().into_owned(),
        text,
        dir: Some(dir),
        example: None,
    })
}

/// The single `*.espf` / `*.espf.txt` file in `dir`.
pub fn find_project(dir: &Path) -> Result<PathBuf, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".espf") || n.ends_with(".espf.txt"))
        .collect();
    names.sort();
    let at = |name: &str| {
        if dir == Path::new(".") {
            PathBuf::from(name)
        } else {
            dir.join(name)
        }
    };
    match names.len() {
        0 => Err(format!(
            "no .espf file in '{}'; give a FILE or --example <category/name>",
            dir.display()
        )),
        1 => Ok(at(&names[0])),
        _ => Err(format!(
            "several .espf files in '{}' ({}); give one explicitly",
            dir.display(),
            names.join(", ")
        )),
    }
}

/// FILE, `--example`, or (neither) the single `.espf` in the current directory.
pub fn resolve_source(file: Option<&Path>, example: Option<&str>) -> Result<Source, String> {
    match (file, example) {
        (Some(_), Some(_)) => Err("give either FILE or --example, not both".to_string()),
        (Some(f), None) => from_file(f),
        (None, None) => from_file(&find_project(Path::new("."))?),
        (None, Some(path)) => {
            let ex = espforge_examples::find(path).ok_or_else(|| {
                format!(
                    "no example '{path}'; run `espforge examples list` to see what is available"
                )
            })?;
            Ok(Source {
                name: format!("embedded:{path}"),
                text: ex.content.clone(),
                dir: None,
                example: Some(ex),
            })
        }
    }
}

// ---------------------------------------------------------------- validation

/// Why validation failed.
#[derive(Debug)]
pub enum Invalid {
    /// Located diagnostics (rendered as snippets).
    Spans(Vec<EspfError>),
    /// A problem with a flag or the like; no source location.
    Message(String),
}

impl From<Vec<EspfError>> for Invalid {
    fn from(e: Vec<EspfError>) -> Self {
        Invalid::Spans(e)
    }
}

impl From<String> for Invalid {
    fn from(m: String) -> Self {
        Invalid::Message(m)
    }
}

impl Invalid {
    pub fn report(&self, name: &str, src: &str) {
        match self {
            Invalid::Spans(errors) => frontend::report(name, src, errors),
            Invalid::Message(m) => eprintln!("❌ {m}"),
        }
    }
}

/// A fully validated spec: final settings (file + flag overrides), resolved
/// options and the bound `$` graph.
pub struct Checked<'a> {
    pub fe: Frontend<'a>,
    /// Core settings after flag overrides.
    pub meta: Meta,
    /// Option answers after flag overrides.
    pub answers: Answers,
    pub resolved: ResolvedOptions,
    pub ctx: RuntimeContext<'a>,
    /// `note: ...` lines for flags that changed something.
    pub notes: Vec<String>,
}

pub fn validate<'a>(src: &'a str, flags: &AnswerArgs) -> Result<Checked<'a>, Invalid> {
    let fe = frontend::load(src)?;

    // Flags win over the file; collect a note when they change something.
    let mut meta = fe.meta.clone();
    let mut answers = fe.answers.clone();
    let mut notes = Vec::new();
    flags.apply_core(&mut meta, &mut notes)?;
    flags.apply_options(
        espforge_options::embedded_schema(),
        &mut answers,
        &mut notes,
    )?;

    let resolved = espforge_options::resolve(&meta, &answers, &fe.derived)?;

    // `$` dependency graph (all five sections exist after `load`).
    let mut ctx = RuntimeContext::new();
    ctx.resolve_and_bind(
        &fe.parsed.sections["peripherals"],
        &fe.parsed.sections["components"],
        &fe.parsed.sections["devices"],
    )?;

    Ok(Checked {
        fe,
        meta,
        answers,
        resolved,
        ctx,
        notes,
    })
}

/// The first section `build` cannot generate firmware for yet.
pub fn unsupported_section(c: &Checked) -> Option<&'static str> {
    ["peripherals", "components", "devices", "services"]
        .into_iter()
        .find(|n| c.fe.parsed.sections.get(*n).is_some_and(|v| !v.is_empty()))
}

fn is_rust_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The hardware `build` can generate code for today: `<gpio>` peripherals
/// used by `<led>` components. Anything else is a clear "not supported yet".
pub fn hardware(c: &Checked) -> Result<Hardware, String> {
    let sections = &c.fe.parsed.sections;
    for name in ["devices", "services"] {
        if sections.get(name).is_some_and(|v| !v.is_empty()) {
            return Err(format!(
                "`# {name}` is not supported by `build` yet (only <gpio> peripherals with <led> components)"
            ));
        }
    }

    // id -> (pin, direction)
    let mut gpios: Vec<(&str, u8, Option<&str>)> = Vec::new();
    for n in sections
        .get("peripherals")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if n.tag_name != "gpio" {
            return Err(format!(
                "<{}> in `# peripherals` is not supported by `build` yet (only <gpio>)",
                n.tag_name
            ));
        }
        let pin = n
            .get_attr("pin")
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| format!("<gpio id=\"{}\"> needs a numeric pin=\"...\"", n.id))?;
        gpios.push((n.id, pin, n.get_attr("direction")));
    }

    let mut leds: Vec<LedSpec> = Vec::new();
    for n in sections.get("components").map(Vec::as_slice).unwrap_or(&[]) {
        if n.tag_name != "led" {
            return Err(format!(
                "<{}> in `# components` is not supported by `build` yet (only <led>)",
                n.tag_name
            ));
        }
        let gpio_ref = n
            .get_attr("gpio")
            .ok_or_else(|| format!("<led id=\"{}\"> needs gpio=\"$<gpio id>\"", n.id))?;
        let target = gpio_ref.trim_start_matches('$');
        let (_, pin, direction) = gpios
            .iter()
            .find(|(id, _, _)| *id == target)
            .ok_or_else(|| {
                format!(
                    "<led id=\"{}\"> refers to `{target}`, which is not a <gpio> in `# peripherals`",
                    n.id
                )
            })?;
        if matches!(direction, Some(d) if *d != "output") {
            return Err(format!(
                "<led id=\"{}\"> needs an output pin, but `{target}` has direction=\"{}\"",
                n.id,
                direction.unwrap_or("")
            ));
        }
        let active_low = match n.get_attr("active_low") {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(format!(
                    "<led id=\"{}\"> active_low=\"{other}\" must be \"true\" or \"false\"",
                    n.id
                ));
            }
        };
        let name = n.id.replace('-', "_");
        if !is_rust_ident(&name) {
            return Err(format!(
                "<led id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ));
        }
        if let Some(other) = leds.iter().find(|l| l.pin == *pin) {
            return Err(format!(
                "<led id=\"{}\"> and <led id=\"{}\"> both use GPIO{pin}",
                other.name, n.id
            ));
        }
        leds.push(LedSpec {
            name,
            pin: *pin,
            active_low,
        });
    }

    Ok(Hardware { leds })
}

/// Whether `build` can turn this spec into firmware. `build` fails on `Err`;
/// `check` reports it as a warning.
pub fn buildable(c: &Checked) -> Result<(), String> {
    hardware(c).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_BUS: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    const WITH_LED: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" active_low=\"true\" />\n# devices\n# services\n";

    fn hello() -> &'static str {
        &espforge_examples::find("01.Basics/helloworld")
            .unwrap()
            .content
    }

    #[test]
    fn find_project_wants_exactly_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_project(dir.path()).is_err());
        std::fs::write(dir.path().join("a.espf"), "").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "").unwrap();
        assert_eq!(find_project(dir.path()).unwrap(), dir.path().join("a.espf"));
        std::fs::write(dir.path().join("b.espf.txt"), "").unwrap();
        let err = find_project(dir.path()).unwrap_err();
        assert!(
            err.contains("a.espf") && err.contains("b.espf.txt"),
            "{err}"
        );
    }

    #[test]
    fn file_and_example_together_are_rejected() {
        assert!(resolve_source(Some(Path::new("x.espf")), Some("01.Basics/helloworld")).is_err());
        assert!(resolve_source(None, Some("no/such")).is_err());
    }

    #[test]
    fn valid_spec_passes_and_is_buildable() {
        let c = validate(hello(), &AnswerArgs::default())
            .ok()
            .expect("hello validates");
        assert!(c.notes.is_empty());
        assert!(buildable(&c).is_ok());
        assert!(hardware(&c).unwrap().leds.is_empty());
    }

    #[test]
    fn flags_override_and_leave_a_note() {
        let flags = AnswerArgs {
            chip: Some("esp32s3".into()),
            ..Default::default()
        };
        let c = validate(hello(), &flags).ok().expect("validates");
        assert_eq!(c.meta.chip, "esp32s3");
        assert_eq!(c.fe.meta.chip, "esp32c3", "the file's own value is kept");
        assert_eq!(c.notes.len(), 1);
    }

    #[test]
    fn bad_flags_are_messages_not_spans() {
        let flags = AnswerArgs {
            chip: Some("esp99".into()),
            ..Default::default()
        };
        assert!(matches!(
            validate(hello(), &flags).err(),
            Some(Invalid::Message(_))
        ));
    }

    #[test]
    fn graph_errors_are_reported_with_spans() {
        let src = WITH_BUS.replace("$i2c0", "$nope");
        match validate(&src, &AnswerArgs::default()).err() {
            Some(Invalid::Spans(errs)) => assert_eq!(errs[0].title, "unknown peripheral"),
            _ => panic!("expected a located error"),
        }
    }

    #[test]
    fn unsupported_hardware_is_valid_but_not_buildable() {
        let c = validate(WITH_BUS, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        assert_eq!(unsupported_section(&c), Some("peripherals"));
        let err = buildable(&c).unwrap_err();
        assert!(err.contains("<i2c>"), "{err}");
    }

    #[test]
    fn gpio_with_a_led_is_buildable_and_becomes_hardware() {
        let c = validate(WITH_LED, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        let hw = hardware(&c).unwrap();
        assert_eq!(
            hw.leds,
            [LedSpec {
                name: "red_led".into(),
                pin: 18,
                active_low: true
            }]
        );
    }

    #[test]
    fn two_leds_on_one_pin_are_refused() {
        let src = WITH_LED.replace(
            "# devices",
            "  <led id=\"other\" gpio=\"$gpio18\" />\n# devices",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("both use GPIO18"));
    }

    #[test]
    fn a_led_on_an_input_pin_is_refused() {
        let src = WITH_LED.replace("output", "input");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("needs an output pin"));
    }
}

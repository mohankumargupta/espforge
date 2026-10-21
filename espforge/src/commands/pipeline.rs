//! The front half shared by `check`, `build` and `gen`: find the source, then
//! parse -> `# espforge` core -> options -> flag overrides -> resolve -> `$`
//! dependency graph. One function, so the commands cannot disagree.

use espf::context::RuntimeContext;
use espf::diag::EspfError;
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

/// Whether `build` can turn this spec into firmware. `build` fails on `Err`;
/// `check` reports it as a warning.
pub fn buildable(c: &Checked) -> Result<(), String> {
    match unsupported_section(c) {
        Some(n) => Err(format!(
            "`# {n}` is not supported by `build` yet (hello-world style projects only)"
        )),
        None => Ok(()),
    }
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
    fn non_empty_sections_are_valid_but_not_buildable() {
        let c = validate(WITH_BUS, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        assert_eq!(unsupported_section(&c), Some("peripherals"));
        assert!(buildable(&c).is_err());
    }
}

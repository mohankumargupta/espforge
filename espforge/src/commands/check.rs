//! `espforge check`: everything `build` validates, without writing anything.
//!
//! Exit codes: 0 valid, 1 problems found (diagnostics already printed).
//!
//! There is one act: [`pipeline::validate`] then [`pipeline::board`]. A spec
//! that lowers to a `Board` is buildable, so `check` never prints `✓ ... is
//! valid` after an error. `--format json` writes one document with the
//! diagnostics (stable codes and spans) and the board.

use clap::Args;
use espforge_diagnostics::{Diagnostic, codes, json::diagnostics_value};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::ExitCode;

use espforge_core::source::resolve_source;
use espforge_core::{board_json, pipeline};

use super::answers::AnswerArgs;
use super::format::{self, Format};
use super::report;

#[derive(Args, Debug, Clone, Default)]
pub struct CheckArgs {
    /// Path to the ESPF file (default: the single .espf in the current directory)
    pub file: Option<PathBuf>,
    /// Check an embedded example instead of a file, e.g. 01.Basics/helloworld
    #[arg(long, value_name = "CATEGORY/NAME", conflicts_with = "file")]
    pub example: Option<String>,
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub format: Format,
    /// Same overrides `build` accepts, so `check` answers "would build accept this?"
    #[command(flatten)]
    pub answers: AnswerArgs,
}

pub fn run(args: CheckArgs) -> ExitCode {
    match execute(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn execute(args: &CheckArgs) -> Result<(), u8> {
    let source = match resolve_source(args.file.as_deref(), args.example.as_deref()) {
        Ok(source) => source,
        Err(message) => {
            match args.format {
                Format::Human => eprintln!("❌ {message}"),
                Format::Json => format::emit(&document(
                    &source_label(args),
                    &[cli_diagnostic(message)],
                    None,
                    &[],
                )),
            }
            return Err(1);
        }
    };

    let checked = match pipeline::validate(&source.text, &args.answers.overrides()) {
        Ok(checked) => checked,
        Err(invalid) => {
            match args.format {
                Format::Human => report::invalid(&invalid, &source.name, &source.text),
                Format::Json => {
                    format::emit(&document(&source.name, &invalid.diagnostics(), None, &[]))
                }
            }
            return Err(1);
        }
    };
    if args.format == Format::Human {
        for note in &checked.notes {
            eprintln!("{note}");
        }
    }

    // The one act (`route` inside): a hardware error is a failure, not a
    // warning: `build` cannot generate this spec, so `check` must not call it
    // valid.
    let board = match pipeline::board(&checked) {
        Ok(board) => board,
        Err(invalid) => {
            match args.format {
                Format::Human => report::invalid(&invalid, &source.name, &source.text),
                Format::Json => format::emit(&document(
                    &source.name,
                    &invalid.diagnostics(),
                    None,
                    &checked.notes,
                )),
            }
            return Err(1);
        }
    };

    match args.format {
        Format::Human => println!("✓ {} is valid", source.name),
        Format::Json => format::emit(&document(
            &source.name,
            &[],
            Some(board_json::value(&board)),
            &checked.notes,
        )),
    }
    Ok(())
}

/// A command-line answer with no source location. The human output keeps the
/// historical one-line `❌`; JSON gets a diagnostic entry.
fn cli_diagnostic(message: String) -> Diagnostic {
    let code = codes::for_cli_message(&message);
    Diagnostic::error(code, 0..0, message, "")
}

fn source_label(args: &CheckArgs) -> String {
    args.file
        .as_ref()
        .map(|path| path.display().to_string())
        .or_else(|| args.example.clone())
        .unwrap_or_else(|| "<no source>".to_string())
}

/// `check`'s machine document: the file, its diagnostics, the board (when one
/// could be lowered) and the notes flags produced. The shape is `check`'s
/// contract; [`format::emit`] only prints it.
fn document(
    file: &str,
    diagnostics: &[Diagnostic],
    board: Option<Value>,
    notes: &[String],
) -> Value {
    json!({
        "file": file,
        "diagnostics": diagnostics_value(file, diagnostics),
        "board": board,
        "notes": notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(name: &str) -> CheckArgs {
        CheckArgs {
            example: Some(name.into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_valid_example_passes() {
        assert!(execute(&example("01.Basics/helloworld")).is_ok());
    }

    #[test]
    fn the_button_example_is_buildable_now() {
        assert!(execute(&example("02.Digital/button")).is_ok());
    }

    #[test]
    fn syntax_errors_fail_with_exit_code_1() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("bad.espf");
        std::fs::write(&f, "# espforge\n  <gpio id=\"a\" pin=\"4 />\n").unwrap();
        let args = CheckArgs {
            file: Some(f),
            ..Default::default()
        };
        assert_eq!(execute(&args), Err(1));
    }

    #[test]
    fn hardware_errors_fail_instead_of_printing_valid() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("pwm.espf");
        std::fs::write(
            &f,
            "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <pwm id=\"pwm0\" />\n# components\n# devices\n# services\n",
        )
        .unwrap();
        let args = CheckArgs {
            file: Some(f),
            ..Default::default()
        };
        assert_eq!(execute(&args), Err(1));
    }

    #[test]
    fn a_pin_conflict_fails_instead_of_printing_valid() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("clash.espf");
        std::fs::write(
            &f,
            "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"g\" pin=\"18\" />\n# components\n  <led id=\"a\" gpio=\"$g\" />\n  <led id=\"b\" gpio=\"$g\" />\n# devices\n# services\n",
        )
        .unwrap();
        let args = CheckArgs {
            file: Some(f),
            ..Default::default()
        };
        assert_eq!(execute(&args), Err(1));
    }

    #[test]
    fn flags_are_checked_like_build_does() {
        let mut args = example("01.Basics/helloworld");
        args.answers.chip = Some("esp99".into());
        assert_eq!(execute(&args), Err(1));
    }

    #[test]
    fn missing_source_is_a_user_error() {
        assert_eq!(execute(&example("no/such")), Err(1));
    }
}

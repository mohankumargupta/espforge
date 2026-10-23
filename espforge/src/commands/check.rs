//! `espforge check`: everything `build` validates, without writing anything.
//!
//! Exit codes: 0 valid, 1 problems found (diagnostics already printed).

use clap::Args;
use espforge_bindings::drivers::generated_registry::route_driver;
use std::path::PathBuf;
use std::process::ExitCode;

use super::answers::AnswerArgs;
use super::pipeline;
use crate::source::resolve_source;

#[derive(Args, Debug, Clone, Default)]
pub struct CheckArgs {
    /// Path to the ESPF file (default: the single .espf in the current directory)
    pub file: Option<PathBuf>,
    /// Check an embedded example instead of a file, e.g. 01.Basics/helloworld
    #[arg(long, value_name = "CATEGORY/NAME", conflicts_with = "file")]
    pub example: Option<String>,
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
    let source =
        resolve_source(args.file.as_deref(), args.example.as_deref()).map_err(|m| {
            eprintln!("❌ {m}");
            1u8
        })?;

    let checked = pipeline::validate(&source.text, &args.answers).map_err(|invalid| {
        invalid.report(&source.name, &source.text);
        1u8
    })?;
    for note in &checked.notes {
        eprintln!("{note}");
    }

    // Driver routing: every device tag must have a compile-time driver.
    for device in &checked.ctx.resolved_devices {
        if let Err(msg) = route_driver(device.tag_name, device, &checked.ctx) {
            eprintln!("{msg}");
            return Err(1);
        }
    }

    // Valid, but say so when `build` cannot generate it yet.
    if let Err(msg) = pipeline::buildable(&checked) {
        eprintln!("warning: {msg}");
    }

    println!("✓ {} is valid", source.name);
    Ok(())
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
    fn unbuildable_but_valid_specs_still_pass_with_a_warning() {
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

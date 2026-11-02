//! `espforge check`: the spec, the app logic and the overlay files, without
//! writing anything.
//!
//! It does not generate a crate, so a passing check does not promise that
//! `build` succeeds: an unsupported overlay placeholder or an edited generated
//! file still fails there. The success line says exactly that.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use espforge_core::project;
use espforge_core::source::resolve_source;

use super::answers::AnswerArgs;
use super::failure::{Failure, exit_code, failed, user};
use super::report;

/// Everything `gen` reads and validates, and nothing it writes.
#[derive(Args, Debug, Clone, Default)]
pub struct CheckArgs {
    /// Path to the ESPF project file or its directory (default: the single .espf in the current directory)
    pub file: Option<PathBuf>,
    /// Check an embedded example instead of a file (category/name, or a short name like blink)
    #[arg(long, value_name = "EXAMPLE", conflicts_with = "file")]
    pub example: Option<String>,
    #[command(flatten)]
    pub answers: AnswerArgs,
}

pub fn run(args: CheckArgs) -> ExitCode {
    exit_code(execute(&args))
}

pub fn execute(args: &CheckArgs) -> Result<(), Failure> {
    let source = resolve_source(
        args.file.as_deref(),
        args.example.as_deref(),
        Path::new("."),
    )
    .map_err(user)?;
    let notes =
        project::check(&source, &args.answers.overrides()).map_err(|e| failed(e, &source))?;
    report::notes(&notes);
    println!(
        "check: '{}' is valid; `espforge build` verifies the generated project",
        source.name
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_examples::find;

    #[test]
    fn an_example_is_valid() {
        let args = CheckArgs {
            example: Some("blink".into()),
            ..Default::default()
        };
        assert!(execute(&args).is_ok());
    }

    #[test]
    fn a_broken_spec_is_a_user_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.espf");
        let hello = find("01.Basics/helloworld").unwrap();
        std::fs::write(&file, hello.content.replace("esp32c3", "esp99")).unwrap();
        let args = CheckArgs {
            file: Some(file),
            ..Default::default()
        };
        assert!(matches!(execute(&args), Err(Failure::User)));
    }

    #[test]
    fn a_bad_override_is_a_user_error() {
        let args = CheckArgs {
            example: Some("blink".into()),
            answers: AnswerArgs {
                options: vec!["nope=1".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(execute(&args), Err(Failure::User)));
    }

    #[test]
    fn missing_app_logic_is_a_user_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.espf");
        let hello = find("01.Basics/helloworld").unwrap();
        std::fs::write(&file, &hello.content).unwrap();
        let args = CheckArgs {
            file: Some(file),
            ..Default::default()
        };
        assert!(matches!(execute(&args), Err(Failure::User)));
    }
}

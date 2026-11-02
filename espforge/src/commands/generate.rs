//! `espforge gen` (`gen` is a keyword, so the module is `generate`): ESPF
//! source -> firmware project, and stop before cargo.
//!
//! Source (FILE, `--example`, or the single `.espf` in the current directory)
//! -> validate -> generate -> write. Everything here is [`espforge_core`]'s;
//! [`execute`] returns what was written and prints nothing, so `build` can run
//! the same step without its output.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use espforge_core::project::{self, Written};
use espforge_core::source::resolve_source;

use super::answers::AnswerArgs;
use super::failure::{Failure, exit_code, failed, user};
use super::report;

#[derive(Args, Debug, Clone, Default)]
pub struct GenArgs {
    /// Path to the ESPF project file or its directory (default: the single .espf in the current directory).
    /// App logic: src/app.rs or app/rust/app.rs next to it
    pub file: Option<PathBuf>,
    /// Use an embedded example instead of a file (category/name, or a short name like blink)
    #[arg(long, value_name = "EXAMPLE", conflicts_with = "file")]
    pub example: Option<String>,
    /// Output directory for the generated project
    #[arg(short, long, default_value = "build")]
    pub out: PathBuf,
    /// Use a local espforge-runtime crate instead of the published one
    /// (or set ESPFORGE_RUNTIME_PATH)
    #[arg(long)]
    pub runtime_path: Option<PathBuf>,
    #[command(flatten)]
    pub answers: AnswerArgs,
}

pub fn run(args: GenArgs) -> ExitCode {
    exit_code(execute(&args).map(|written| {
        report::notes(&written.notes);
        println!(
            "gen: wrote '{}' to {}",
            written.crate_name,
            args.out.display()
        );
        println!("  next: espforge build");
    }))
}

/// Generate the project and say nothing: the caller owns the terminal.
pub fn execute(args: &GenArgs) -> Result<Written, Failure> {
    let source = resolve_source(
        args.file.as_deref(),
        args.example.as_deref(),
        Path::new("."),
    )
    .map_err(user)?;
    project::generate_with(
        &source,
        &args.out,
        &args.answers.overrides(),
        args.runtime_path.clone(),
    )
    .map_err(|e| failed(e, &source))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn example_args(example: &str, out: &Path) -> GenArgs {
        GenArgs {
            example: Some(example.into()),
            out: out.to_path_buf(),
            ..Default::default()
        }
    }

    fn is_user<T: std::fmt::Debug>(r: Result<T, Failure>) -> bool {
        matches!(r, Err(Failure::User))
    }

    #[test]
    fn generates_the_helloworld_example() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let written = execute(&example_args("01.Basics/helloworld", &out)).unwrap();
        assert_eq!(written.crate_name, "hello_world");
        for f in [
            "Cargo.toml",
            "build.rs",
            "src/lib.rs",
            "src/bin/main.rs",
            "src/app.rs",
            "README.txt",
            ".espforge/manifest.json",
            ".espforge/source.espf.txt",
        ] {
            assert!(out.join(f).is_file(), "missing {f}");
        }
        assert!(
            std::fs::read_to_string(out.join("src/app.rs"))
                .unwrap()
                .contains("Hello World!")
        );
    }

    #[test]
    fn a_short_example_name_generates_too() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let written = execute(&example_args("blink", &out)).unwrap();
        assert!(!written.crate_name.is_empty());
        assert!(out.join("Cargo.toml").is_file());
    }

    #[test]
    fn rebuilding_is_fine_but_edited_owned_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let args = example_args("01.Basics/helloworld", &out);
        execute(&args).unwrap();
        execute(&args).unwrap();
        std::fs::write(out.join("src/lib.rs"), "// mine").unwrap();
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn flags_override_the_file_and_leave_notes() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let mut args = example_args("01.Basics/helloworld", &out);
        args.answers.chip = Some("esp32s3".into());
        let written = execute(&args).unwrap();
        assert_eq!(written.notes.len(), 1);
        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("\"esp32s3\""), "{cargo}");
    }

    #[test]
    fn bad_flags_are_user_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut args = example_args("01.Basics/helloworld", &dir.path().join("out"));
        args.answers.options = vec!["nope=1".into()];
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn spec_errors_are_user_errors_with_snippets() {
        let dir = tempfile::tempdir().unwrap();
        let hello = espforge_examples::find("01.Basics/helloworld").unwrap();
        let f = dir.path().join("hello.espf");
        std::fs::write(&f, hello.content.replace("esp32c3", "esp99")).unwrap();
        let args = GenArgs {
            file: Some(f),
            out: dir.path().join("out"),
            ..Default::default()
        };
        assert!(is_user(execute(&args)));
    }

    #[test]
    fn file_and_example_together_or_a_missing_example_are_user_errors() {
        let both = GenArgs {
            file: Some("x.espf".into()),
            example: Some("01.Basics/helloworld".into()),
            ..Default::default()
        };
        assert!(is_user(execute(&both)));
        let missing = GenArgs {
            example: Some("no/such".into()),
            ..Default::default()
        };
        assert!(is_user(execute(&missing)));
    }

    #[test]
    fn runtime_path_is_validated_and_used() {
        let dir = tempfile::tempdir().unwrap();
        let rt = dir.path().join("rt");
        std::fs::create_dir_all(&rt).unwrap();
        let out = dir.path().join("out");
        let mut args = example_args("01.Basics/helloworld", &out);
        args.runtime_path = Some(rt.clone());
        execute(&args).unwrap();
        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains(&rt.to_string_lossy().replace('\\', "/")),
            "{cargo}"
        );

        args.runtime_path = Some(dir.path().join("not-a-dir"));
        assert!(is_user(execute(&args)));
    }
}

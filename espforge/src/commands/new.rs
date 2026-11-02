//! `espforge new`: an embedded example as an editable project.
//!
//! Resolving the name and writing the tree belong to `espforge_examples`; this
//! command only turns its typed failure into the shared exit codes. A conflict
//! (the destination is taken) is the user's mistake; an OS failure is ours.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use espforge_examples::{MaterializeError, materialize, resolve_name};

use super::failure::{Failure, exit_code, user};

#[derive(Args, Debug, Clone)]
pub struct NewArgs {
    /// Example to scaffold: a name (`blink`) or a category key (`01.Basics/blink`)
    pub example: String,
    /// New directory for the project (must be absent or empty)
    pub dir: PathBuf,
}

pub fn run(args: NewArgs) -> ExitCode {
    exit_code(execute(&args))
}

pub fn execute(args: &NewArgs) -> Result<(), Failure> {
    let (_, example) = resolve_name(&args.example).map_err(user)?;
    let done = materialize(example, &args.dir).map_err(|error| match error {
        MaterializeError::Conflict(message) => user(message),
        MaterializeError::Io(error) => {
            Failure::Internal(format!("cannot write '{}': {error}", args.dir.display()))
        }
    })?;

    println!(
        "new: created '{}' in {}",
        done.spec.file_name().unwrap_or_default().to_string_lossy(),
        args.dir.display()
    );
    println!();
    println!("Next steps:");
    println!("  cd {}", args.dir.display());
    println!("  espforge build");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use espforge_core::project;
    use espforge_core::source::resolve_source;

    use super::*;

    fn args(example: &str, dir: &Path) -> NewArgs {
        NewArgs {
            example: example.into(),
            dir: dir.to_path_buf(),
        }
    }

    #[test]
    fn scaffolds_a_project_that_generates() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("my-app");
        execute(&args("blink", &dir)).unwrap();

        assert!(dir.join("blink.espf").is_file());
        assert!(dir.join("src").join("app.rs").is_file());
        assert!(dir.join("diagram.json").is_file());
        assert!(!dir.join("example.toml").exists(), "harness file leaked");
        assert!(!dir.join("app").exists(), "example layout leaked");

        let spec = std::fs::read_to_string(dir.join("blink.espf")).unwrap();
        assert!(spec.contains("# espforge"), "{spec}");
        assert!(spec.contains("BLINK EXAMPLE"), "{spec}");

        // The scaffold and the discovery agree about where the spec and the app
        // logic live: generating the new project is the proof.
        let source = resolve_source(None, None, &dir).unwrap_or_else(|e| panic!("{e}"));
        project::generate(&source, &dir.join("build")).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(dir.join("build/Cargo.toml").is_file());
        assert!(dir.join("build/src/app.rs").is_file());
    }

    #[test]
    fn the_scaffolded_project_passes_check() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("my-app");
        execute(&args("blink", &dir)).unwrap();

        let checked = crate::commands::check::execute(&crate::commands::check::CheckArgs {
            file: Some(dir.join("blink.espf")),
            ..Default::default()
        });
        assert!(checked.is_ok(), "check must accept a scaffolded project");
    }

    #[test]
    fn a_full_category_key_scaffolds_too() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("hello");
        execute(&args("01.Basics/helloworld", &dir)).unwrap();
        assert!(dir.join("helloworld.espf").is_file());
    }

    #[test]
    fn a_non_empty_directory_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("taken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep.txt"), "mine").unwrap();

        assert!(matches!(execute(&args("blink", &dir)), Err(Failure::User)));
        assert!(!dir.join("blink.espf").exists(), "nothing may be written");
    }

    #[test]
    fn an_unknown_example_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("typo");
        assert!(matches!(execute(&args("blinkk", &dir)), Err(Failure::User)));
        assert!(!dir.exists());
    }
}

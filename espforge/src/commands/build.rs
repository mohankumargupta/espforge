//! `espforge build`: `gen`, then `cargo build` in the generated project.
//!
//! Exit codes match `gen`'s (0 success, 1 user, 2 internal). A failing `cargo`
//! is the user's: either the spec produced code that does not compile on their
//! toolchain, or the toolchain is missing from their environment.

use std::path::Path;
use std::process::{Command, ExitCode};

use clap::Args;

use super::failure::{Failure, exit_code, user};
use super::generate::{self, GenArgs};
use super::report;

#[derive(Args, Debug, Clone, Default)]
pub struct BuildArgs {
    #[command(flatten)]
    pub generate: GenArgs,
    /// Compile with optimizations (`cargo build --release`)
    #[arg(long)]
    pub release: bool,
}

pub fn run(args: BuildArgs) -> ExitCode {
    exit_code(execute(&args))
}

fn execute(args: &BuildArgs) -> Result<(), Failure> {
    let written = generate::execute(&args.generate)?;
    report::notes(&written.notes);
    println!(
        "build: wrote '{}' to {}",
        written.crate_name,
        args.generate.out.display()
    );
    cargo_build(&args.generate.out, args.release)
}

fn cargo_build(out: &Path, release: bool) -> Result<(), Failure> {
    let mut command = Command::new("cargo");
    command.arg("build").current_dir(out);
    if release {
        command.arg("--release");
    }

    let status = command.status().map_err(|e| {
        Failure::Internal(format!(
            "could not run `cargo` ({e}). Is Rust installed and on your PATH?"
        ))
    })?;

    if status.success() {
        println!("build: finished");
        Ok(())
    } else {
        Err(user("cargo build failed (see the errors above)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_source_stops_before_cargo() {
        let mut args = BuildArgs {
            generate: GenArgs {
                example: Some("no/such".into()),
                ..Default::default()
            },
            release: false,
        };
        assert!(matches!(execute(&args), Err(Failure::User)));

        args.generate.example = Some("01.Basics/helloworld".into());
        args.generate.answers.options = vec!["nope=1".into()];
        assert!(matches!(execute(&args), Err(Failure::User)));
    }
}

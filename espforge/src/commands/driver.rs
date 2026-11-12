//! `espforge driver`: record, fetch, verify, migrate, list and forget driver
//! manifests.
//!
//! Generation is offline: it reads `drivers/`, `--driver-dir` and the cache,
//! and verifies `.espforge/drivers.lock`. This command is the only one that
//! writes the lock or fetches a package.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use espforge_core::drivers;

#[derive(Args, Debug)]
pub struct DriverArgs {
    /// Project file or directory (default: the current directory)
    pub file: Option<PathBuf>,
    #[command(subcommand)]
    pub command: DriverCommand,
}

#[derive(Subcommand, Debug)]
pub enum DriverCommand {
    /// Add a driver from a path, an HTTPS archive, a Git revision or a registry
    Add {
        /// A `.kdl` path, `https://…tar.gz`, `git+https://…#<rev>` or `<tag>@<version>`
        source: String,
        /// Record a local manifest where it is instead of copying it into `drivers/`
        #[arg(long)]
        local: bool,
        /// Do not ask for confirmation (the default: this command never prompts)
        #[arg(long)]
        yes: bool,
        /// Pin an archive's SHA-256 before extraction
        #[arg(long)]
        sha256: Option<String>,
        /// The revision for a `git+…` source
        #[arg(long)]
        rev: Option<String>,
        /// A static registry index URL, for `<tag>@<version>`
        #[arg(long)]
        registry: Option<String>,
        /// Allow `file://` and `http://` sources (test fixtures only)
        #[arg(long)]
        allow_local_source: bool,
        /// Load and validate without writing anything
        #[arg(long)]
        dry_run: bool,
        /// The cache version for an archive or Git source
        #[arg(long)]
        version: Option<String>,
    },
    /// List the recorded drivers: tag, kind, crate, source and short hash
    List,
    /// Refresh a local hash, or re-fetch a recorded remote source
    Update {
        /// The tag to refresh (default: every recorded driver)
        tag: Option<String>,
        /// Allow `file://` and `http://` sources when re-fetching
        #[arg(long)]
        allow_local_source: bool,
    },
    /// Forget a driver and delete its copy in `drivers/`
    Remove {
        tag: String,
        /// Delete the cache entry when no lock entry refers to it
        #[arg(long)]
        prune_cache: bool,
    },
    /// Re-hash cached drivers, or re-fetch them with `--refresh`
    Verify {
        /// Re-fetch every recorded remote source
        #[arg(long)]
        refresh: bool,
        /// Allow `file://` and `http://` sources when re-fetching
        #[arg(long)]
        allow_local_source: bool,
    },
    /// Rewrite a v1 `element` manifest as a v2 `driver` file
    Migrate {
        /// The v1 manifest to convert
        file: PathBuf,
        /// Write the converted file in place instead of printing it
        #[arg(long)]
        write: bool,
    },
}

pub fn run(args: DriverArgs) -> ExitCode {
    let dir = project_dir(args.file.as_deref());
    match execute(&dir, &args.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("❌ {message}");
            ExitCode::FAILURE
        }
    }
}

fn project_dir(file: Option<&Path>) -> PathBuf {
    match file {
        Some(path) if path.is_dir() => path.to_path_buf(),
        Some(path) => path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
        None => PathBuf::from("."),
    }
}

fn execute(dir: &Path, command: &DriverCommand) -> Result<(), String> {
    match command {
        DriverCommand::Add {
            source,
            local,
            yes,
            sha256,
            rev,
            registry,
            allow_local_source,
            dry_run,
            version,
        } => {
            let _ = yes;
            println!(
                "{}",
                drivers::add_spec(
                    dir,
                    source,
                    rev.as_deref(),
                    registry.as_deref(),
                    *allow_local_source,
                    *local,
                    sha256.as_deref(),
                    version.as_deref(),
                    *dry_run,
                )?
            );
        }
        DriverCommand::List => {
            let entries = drivers::list(dir)?;
            if entries.is_empty() {
                println!(
                    "no drivers recorded in {}",
                    drivers::lock_path(dir).display()
                );
            }
            for entry in entries {
                println!("{entry}");
            }
        }
        DriverCommand::Update {
            tag,
            allow_local_source,
        } => {
            for line in drivers::update(dir, tag.as_deref(), *allow_local_source)? {
                println!("updated {line}");
            }
        }
        DriverCommand::Remove { tag, prune_cache } => {
            drivers::remove(dir, tag, *prune_cache)?;
            println!("removed `<{tag}>`");
        }
        DriverCommand::Verify {
            refresh,
            allow_local_source,
        } => {
            for line in drivers::verify_cache(dir, *refresh, *allow_local_source)? {
                println!("{line}");
            }
        }
        DriverCommand::Migrate { file, write } => {
            let text = drivers::migrate(file)?;
            if *write {
                std::fs::write(file, &text)
                    .map_err(|error| format!("cannot write '{}': {error}", file.display()))?;
                println!("migrated '{}'", file.display());
            } else {
                print!("{text}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_path_resolves_to_its_directory() {
        assert_eq!(
            project_dir(Some(Path::new("work/app.espf"))),
            PathBuf::from("work")
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(project_dir(Some(dir.path())), dir.path().to_path_buf());
        assert_eq!(project_dir(None), PathBuf::from("."));
    }

    #[test]
    fn the_driver_subcommands_include_fetch_and_migration() {
        let command = <DriverArgs as clap::Args>::augment_args(clap::Command::new("driver"));
        let names: Vec<&str> = command
            .get_subcommands()
            .map(|command| command.get_name())
            .collect();
        assert_eq!(
            names,
            ["add", "list", "update", "remove", "verify", "migrate"]
        );
    }
}

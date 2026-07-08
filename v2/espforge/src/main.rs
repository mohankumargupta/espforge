//! espforge v2 CLI.
//!
//! Subcommands (ADR-011): `version`, `validate`, `build`.
//! - `version`  — print the espforge version.
//! - `validate` — parse + run the `validate` stage on a project YAML and report
//!                diagnostics without emitting a project (ADR-009). In this
//!                phase it parses and reports shape errors; the full semantic
//!                validation lands in Phase 3.
//! - `build`    — parse + emit a project. Pipeline completed in later phases.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "espforge", version, about = "Declare a board, get correct no_std firmware")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the espforge version.
    Version,
    /// Validate a project YAML and report diagnostics without emitting.
    Validate {
        /// Path to the project YAML.
        project: PathBuf,
    },
    /// Generate a firmware project from a project YAML.
    Build {
        /// Path to the project YAML.
        project: PathBuf,
        /// Output directory for the generated project.
        #[arg(short, long, default_value = "build")]
        out: PathBuf,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("espforge {VERSION}");
        }
        Command::Validate { project } => {
            let text = std::fs::read_to_string(&project)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", project.display()))?;
            let proj = espforge::parse::parse_str(&text)?;
            match espforge::pipeline::validate(&proj) {
                Ok(()) => {
                    println!(
                        "validate: OK — {} component(s), {} device(s)",
                        proj.components.len(),
                        proj.devices.len()
                    );
                }
                Err(diags) => {
                    for d in &diags {
                        eprintln!("{}", d.render(&text));
                    }
                    eprintln!("validate: {} error(s)", diags.len());
                    std::process::exit(1);
                }
            }
        }
        Command::Build { project, out } => {
            let text = std::fs::read_to_string(&project)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", project.display()))?;
            let proj = espforge::parse::parse_str(&text)?;
            espforge::pipeline::validate(&proj)
                .map_err(|diags| anyhow::anyhow!("validation failed: {} error(s)", diags.len()))?;
            let ir = espforge::pipeline::resolve(&proj);

            // Layer 1: standard Espressif scaffold (esp-generate), merged without
            // clobbering espforge's files.
            espforge::emit::scaffold::scaffold(&ir, &out)
                .map_err(|e| anyhow::anyhow!("scaffold failed: {e}"))?;

            // Layer 2: espforge's wiring layers + manifest.
            let artifacts = espforge::emit::generate(&ir)?;
            espforge::emit::write(&out, &artifacts)?;

            println!(
                "build: wrote project to {} — {} instance(s), features {:?}",
                out.display(),
                ir.instances.len(),
                ir.required_features
            );
            println!("  (run `cargo build` in that dir with an esp toolchain to compile)");
        }
    }
    Ok(())
}

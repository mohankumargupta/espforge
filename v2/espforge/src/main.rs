//! espforge v2 CLI.
//!
//! Subcommands (design §17): `create`/`setup`, `build`, `validate`, `version`.
//! - `create`/`setup` — bootstrap a project from a baked-in template ONCE
//!   (copies assets; does not emit). The clean-and-jerk's "clean" half.
//! - `build`    — parse + emit a project. Repeatable (arg-less in cwd). The
//!   "jerk" half.
//! - `validate` — parse + run the `validate` stage, report diagnostics only.

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
    /// Create a new project from a template (runs once).
    Create {
        /// Template name (e.g. `blink`). Omit to pick interactively.
        example: Option<String>,
        /// Project (folder) name. Defaults to the example name.
        #[arg(short, long)]
        name: Option<String>,
        /// Directory to create the project in. Defaults to the current dir.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Alias for `create`.
    Setup {
        /// Template name (e.g. `blink`). Omit to pick interactively.
        example: Option<String>,
        /// Project (folder) name. Defaults to the example name.
        #[arg(short, long)]
        name: Option<String>,
        /// Directory to create the project in. Defaults to the current dir.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Generate a firmware project from a spec.
    ///
    /// With no `--project`, discovers the spec in the current directory
    /// (design §17.2).
    Build {
        /// Path to the project YAML. Defaults to discovery in `OUT`/cwd.
        #[arg(short, long)]
        project: Option<PathBuf>,
        /// Output directory for the generated project. Defaults to cwd.
        #[arg(short, long, default_value = "build")]
        out: PathBuf,
    },
    /// Validate a project YAML and report diagnostics without emitting.
    Validate {
        /// Path to the project YAML.
        project: PathBuf,
    },
    /// Print the espforge version.
    Version,
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
        Command::Create {
            example,
            name,
            out,
        }
        | Command::Setup {
            example,
            name,
            out,
        } => {
            run_create(example, name, out)?;
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
            // Design §17.2: arg-less build discovers the spec in the current dir,
            // while output still goes to `out`.
            let project = match project {
                Some(p) => p,
                None => discover_spec(std::path::Path::new("."))?,
            };
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

/// Design §17.2: find the project spec in `dir` when `--project` is omitted.
/// The spec is the YAML in `dir` whose top-level mapping contains `espforge:`
/// and `components:`/`devices:`. Returns the first match (explicit over
/// implicit: we pick the unambiguous spec, not a guessed filename).
fn discover_spec(dir: &std::path::Path) -> anyhow::Result<PathBuf> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("failed to read directory {}: {e}", dir.display()))?;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.contains("espforge:") && (text.contains("components:") || text.contains("devices:")) {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "no espforge spec found in {} — run `create` first, or pass --project <path>",
        dir.display()
    )
}

/// Design §17.1: bootstrap a project folder from a baked-in template.
fn run_create(
    example: Option<String>,
    name: Option<String>,
    out: Option<PathBuf>,
) -> anyhow::Result<()> {
    // 1. Resolve the example name (arg, else interactive dialoguer picker).
    let example = match example {
        Some(name) => name,
        None => prompt_example()?,
    };

    // 2. Closed-set lookup — unknown name -> error + exit (no fuzzy match).
    if !espforge_examples::is_example(&example) {
        anyhow::bail!(
            "unknown example `{example}`\nknown examples: {}",
            espforge_examples::example_names().join(", ")
        );
    }

    // 3. Project name defaults to the example's leaf name (the part after the
    //    last `/`), so `create 01.Basics/blink` and `create blink` both yield a
    //    `blink/` project folder. The category prefix is just for selection.
    let name = name.unwrap_or_else(|| {
        example
            .rsplit('/')
            .next()
            .unwrap_or(&example)
            .to_string()
    });
    let out = out.unwrap_or_else(|| PathBuf::from("."));
    let dest = out.join(&name);
    if dest.exists() {
        anyhow::bail!("destination `{}` already exists", dest.display());
    }

    // 4. Copy template assets: <example>.yaml -> <name>.yaml, app/rust/app.rs ->
    //    src/app.rs, diagram.json (optional).
    std::fs::create_dir_all(dest.join("src"))
        .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", dest.display()))?;
    copy_spec(&example, &name, &dest)?;
    copy_app(&example, &dest)?;
    // diagram.json is optional; only copy if the template ships one.
    if let Some(d) = espforge_examples::asset(&example, "diagram.json") {
        std::fs::write(dest.join("diagram.json"), d)
            .map_err(|e| anyhow::anyhow!("failed to write diagram.json: {e}"))?;
    }

    // 5. Friendly, explicit next steps (v1 `example` behaviour).
    println!("created project `{name}` at {}", dest.display());
    println!();
    println!("  edit:");

    let app_path = if espforge_examples::asset(&example, "app/rust/app.rs").is_some() {
        format!("    {}/src/app.rs", dest.display())
    } else {
        format!("    {}/src/app.rs  (then run `espforge build` to scaffold it)", dest.display())
    };
    println!("{app_path}");
    println!("    {}/{}", dest.display(), name);
    if espforge_examples::asset(&example, "diagram.json").is_some() {
        println!("    {}/diagram.json", dest.display());
    }
    println!();
    println!("  then build it:");
    let proj_arg = format!("{}", dest.join(format!("{name}.yaml")).display());
    println!("    cd {name} && espforge build --project {} --out build", proj_arg);
    println!();
    println!("  `build` is repeatable — re-run it after any YAML or app.rs edit.");
    Ok(())
}

/// Interactive picker (dialoguer). Triggered only when the example arg is
/// omitted. No full-screen TUI — a flat select + name input (design §17.1).
fn prompt_example() -> anyhow::Result<String> {
    let names = espforge_examples::example_names();
    let choice = dialoguer::Select::new()
        .with_prompt("Choose an example template")
        .items(&names)
        .default(0)
        .interact_opt()
        .map_err(|e| anyhow::anyhow!("prompt failed: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("cancelled"))?;
    Ok(names[choice].to_string())
}

/// Copy the example's spec (any embedded `.yaml` containing `espforge:`) to
/// `<dest>/<name>.yaml`. The destination is always named after the project.
fn copy_spec(example: &str, name: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let spec = espforge_examples::example_spec(example)
        .ok_or_else(|| anyhow::anyhow!("template `{example}` has no project spec"))?;
    let dest_path = dest.join(format!("{name}.yaml"));
    std::fs::write(dest_path, spec)
        .map_err(|e| anyhow::anyhow!("failed to write spec: {e}"))?;
    Ok(())
}

/// Copy `<example>/app/rust/app.rs` to `<dest>/src/app.rs` (user-owned).
fn copy_app(example: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let app = espforge_examples::asset(example, "app/rust/app.rs")
        .ok_or_else(|| anyhow::anyhow!("template `{example}` is missing app/rust/app.rs"))?;
    let dest_path = dest.join("src").join("app.rs");
    std::fs::write(dest_path, app)
        .map_err(|e| anyhow::anyhow!("failed to write src/app.rs: {e}"))?;
    Ok(())
}

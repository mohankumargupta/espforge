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
            // Honour persisted `answers.yaml` (from `create`/`setup`) when run
            // directly. Env vars (e.g. set via `just`) take precedence.
            let profile = if std::env::var("ESPFORGE_USE_LOCAL").is_err() {
                let settings = read_settings(
                    project.parent().unwrap_or_else(|| std::path::Path::new(".")),
                );
                let profile = settings
                    .as_ref()
                    .map(|s| match s.debug_or_release {
                        Profile::Release => "release",
                        Profile::Debug => "debug",
                    })
                    .unwrap_or("debug");
                if let Some(settings) = settings {
                    if settings.use_local {
                        // SAFETY: single-threaded CLI startup; set once before emit.
                        unsafe { std::env::set_var("ESPFORGE_USE_LOCAL", "true") };
                        // `path:` in answers.yaml is relative to the project dir
                        // (parent of the spec). The generated manifest lives in
                        // `out` (e.g. `build/`, one level deeper), so re-base the
                        // checkout root to be relative to `out` and hand it to the
                        // emitter via ESPFORGE_PATH (which takes precedence over
                        // deriving the root from the espforge binary location).
                        let project_dir =
                            project.parent().unwrap_or_else(|| std::path::Path::new("."));
                        let checkout = project_dir.join(&settings.path);
                        if let Some(rel) = diff_paths(&checkout, &out) {
                            unsafe { std::env::set_var("ESPFORGE_PATH", rel) };
                        }
                    }
                }
                profile
            } else {
                "debug"
            };
            let text = std::fs::read_to_string(&project)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", project.display()))?;
            let proj = espforge::parse::parse_str(&text)?;
            if let Err(diags) = espforge::pipeline::validate(&proj) {
                for d in &diags {
                    eprintln!("{}", d.render(&text));
                }
                anyhow::bail!("validation failed: {} error(s)", diags.len());
            }
            let ir = espforge::pipeline::resolve(&proj);

            // Layer 1: standard Espressif scaffold (esp-generate), merged without
            // clobbering espforge's files.
            espforge::emit::scaffold::scaffold(&ir, &out)
                .map_err(|e| anyhow::anyhow!("scaffold failed: {e}"))?;

            // Layer 2: espforge's wiring layers + manifest.
            let artifacts = espforge::emit::generate(&ir, &out)?;
            espforge::emit::write(&out, &artifacts)?;

            // Layer 3: wokwi simulation assets. `diagram.json` is copied from
            // the project root with placeholders resolved; `wokwi.toml` is
            // generated pointing at the compiled binary. Both are always
            // overwritten in `out` (generated output).
            let project_dir = project.parent().unwrap_or_else(|| std::path::Path::new("."));
            espforge::emit::wokwi::resolve_diagram(project_dir, &out, &ir)?;
            // Wokwi custom chip: `setup` staged `<project>/chip/`; carry it into
            // `out/chip/` and emit a `[[chip]]` section if present. The chip's
            // `name` (from `chip.json`) becomes wokwi's part-type prefix.
            let chip_name = copy_chip_to_out(project_dir, &out)?;
            espforge::emit::wokwi::write_wokwi_toml(&out, &ir, profile, chip_name.as_deref())?;

            // Layer 4: carry the user-owned `src/app.rs` verbatim from the
            // project into `out/src/app.rs` so user edits are reflected in the
            // build. Always overwritten (it is user input, not generated code).
            copy_app_to_out(project_dir, &out)?;

            // Report the features actually emitted into the generated manifest:
            // `required_features` are project-level (embassy/alloc/wifi) and
            // `runtime_features` are the `espforge-runtime` module features
            // gating the components in use. The earlier log only printed
            // `required_features`, which is empty for blocking examples and made
            // the gating look broken. Each driver's `runtime_features()`
            // defaults to `[kind()]` (see `driver.rs`), so the instance kind is
            // exactly the emitted module feature.
            let mut runtime_features: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for inst in &ir.instances {
                runtime_features.insert(inst.kind.clone());
            }
            println!(
                "build: wrote project to {} — {} instance(s), runtime features {:?}",
                out.display(),
                ir.instances.len(),
                runtime_features
            );
            println!("  (the scaffold step above printed the exact `esp-generate` command used)");
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
    // 0. Read the optional `answers.yaml` from the cwd — it controls the
    //    generated justfile (binary path, local crates, build profile).
    let answers = read_answers(std::path::Path::new("."));

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
    // Write the justfile derived from answers.yaml (ADR-001) at the project
    // root, next to `answers.yaml`.
    write_justfile(&dest, &answers)?;
    // Carry the answers down into the project as `answers.yaml` at the project
    // root, wrapped in `espforge:`. `espforge build` reads this so it honours
    // `use_local` when run directly rather than through `just`. `config`
    // records the spec (project YAML) file name so it travels with the project.
    let spec_name = format!("{name}.yaml");
    write_settings(&dest, &answers, &spec_name)?;
    // diagram.json / wokwi.toml are optional; copy verbatim if the template
    // ships them (build resolves placeholders and regenerates them into `out`).
    copy_asset(&example, "diagram.json", &dest)?;
    copy_asset(&example, "wokwi.toml", &dest)?;
    // Wokwi custom-chip assets: copy only the runtime artifacts
    // (`chip/chip.json` + `chip/chip.wasm`) into `<project>/chip/`. The Zig
    // build sources stay in the example tree; build re-carries `chip/` into
    // `out/` and emits the `[[chip]]` section of `wokwi.toml`.
    copy_chip_assets(&example, &dest)?;

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
    if espforge_examples::asset(&example, "wokwi.toml").is_some() {
        println!("    {}/wokwi.toml", dest.display());
    }
    if espforge_examples::asset(&example, "chip.wasm").is_some() {
        println!("    {}/chip/  (Wokwi custom chip: chip.json + chip.wasm)", dest.display());
    }
    println!();
    println!("  then build it:");
    let proj_arg = format!("{}", dest.join(format!("{name}.yaml")).display());
    println!("    cd {name} && espforge build --project {} --out build", proj_arg);
    println!("    (or: cd {name} && just build)");
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

/// Copy `<example>/app/rust/app.rs` to `<dest>/src/app.rs` (user-owned). If the
/// template ships no app.rs, scaffold a minimal skeleton so the project still
/// compiles. `build` later copies the project's `src/app.rs` verbatim into
/// `out/src/app.rs`, so user edits are always carried through.
fn copy_app(example: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let app = espforge_examples::asset(example, "app/rust/app.rs")
        .unwrap_or_else(|| APP_SKELETON.as_bytes());
    let dest_path = dest.join("src").join("app.rs");
    std::fs::write(dest_path, app)
        .map_err(|e| anyhow::anyhow!("failed to write src/app.rs: {e}"))?;
    Ok(())
}

/// Copy a Wokwi custom chip's runtime artifacts from the embedded example tree
/// into `<dest>/chip/`. Only `chip/chip.json` + `chip/chip.wasm` travel — the
/// Zig build sources (`chip.zig`, `build.zig`, `justfile`, `wokwi-api.zig`) are
/// intentionally left in the example tree (the committed `.wasm` is the build
/// product). No-op if the example ships no chip. The chip dir is the signal
/// `build` uses to emit the `[[chip]]` section of `wokwi.toml` and to re-carry
/// `chip/` into `out/`.
fn copy_chip_assets(example: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    // The runtime artifacts (`chip.json` + `chip.wasm`) live at the example
    // root; the Zig build sources are under `chip/`. Only the artifacts travel.
    let json = espforge_examples::asset(example, "chip.json");
    let wasm = espforge_examples::asset(example, "chip.wasm");
    let (Some(json), Some(wasm)) = (json, wasm) else {
        return Ok(()); // example has no custom chip — nothing to do.
    };
    let chip_dir = dest.join("chip");
    std::fs::create_dir_all(&chip_dir)
        .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", chip_dir.display()))?;
    std::fs::write(chip_dir.join("chip.json"), json)
        .map_err(|e| anyhow::anyhow!("failed to write chip.json: {e}"))?;
    std::fs::write(chip_dir.join("chip.wasm"), wasm)
        .map_err(|e| anyhow::anyhow!("failed to write chip.wasm: {e}"))?;
    Ok(())
}

/// Minimal user-owned `app.rs` scaffold used when an example ships no
/// `app/rust/app.rs`. The blocking `setup`/`forever` hooks match the default
/// (non-embassy) runtime; embassy examples are expected to ship their own.
const APP_SKELETON: &str = r#"// USER-OWNED. This file is NOT regenerated by espforge. Edit freely.
pub fn setup(_ctx: &mut crate::Context) {
    // espforge wired your components/devices; access them via `component!`/`device!`.
}

pub fn forever(_ctx: &mut crate::Context) {
    // your loop body here
}
"#;

/// Copy the project's user-owned `src/app.rs` verbatim into `out/src/app.rs`
/// so edits made in the project are carried through to the build. The project
/// copy is created by `setup` (from the example, or a scaffold fallback), so it
/// always exists; if somehow absent we leave `out` without it.
fn copy_app_to_out(project_dir: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let src = project_dir.join("src").join("app.rs");
    if !src.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(&src)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", src.display()))?;
    let dest_dir = out.join("src");
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", dest_dir.display()))?;
    std::fs::write(dest_dir.join("app.rs"), bytes)
        .map_err(|e| anyhow::anyhow!("failed to write out/src/app.rs: {e}"))?;
    Ok(())
}

/// Carry a Wokwi custom chip from the project into `out/chip/` (recursive, so
/// any file `setup` staged travels). Returns the chip's `name` parsed from
/// `chip.json` (used by `write_wokwi_toml` to emit the `[[chip]]` section), or
/// `None` if the project has no chip. `build` uses this as the chip-presence
/// signal.
fn copy_chip_to_out(
    project_dir: &std::path::Path,
    out: &std::path::Path,
) -> anyhow::Result<Option<String>> {
    let src = project_dir.join("chip");
    if !src.is_dir() {
        return Ok(None);
    }
    let dst = out.join("chip");
    std::fs::create_dir_all(&dst)
        .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", dst.display()))?;
    copy_dir_recursive(&src, &dst)
        .map_err(|e| anyhow::anyhow!("failed to copy chip/ into out: {e}"))?;
    // Read the chip name (wokwi requires the diagram part type `chip-<name>`
    // to match). Tolerate a missing/invalid json by skipping the `[[chip]]`.
    let name = std::fs::read_to_string(src.join("chip.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()));
    Ok(name)
}

/// Recursively copy a directory tree, preserving nested structure. Used to
/// carry a Wokwi chip's `chip/` folder (json + wasm, and any future assets)
/// into the build output verbatim.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_recursive(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Copy an optional example asset verbatim into the project folder, if the
/// template ships it. Used for `diagram.json` / `wokwi.toml` (both optional).
fn copy_asset(example: &str, name: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    if let Some(bytes) = espforge_examples::asset(example, name) {
        std::fs::write(dest.join(name), bytes)
            .map_err(|e| anyhow::anyhow!("failed to write {name}: {e}"))?;
    }
    Ok(())
}

/// Options that control how `setup`/`create` bootstrap a project (design
/// §17.1). `use_local` selects local espforge crates vs published crates.io;
/// `path` is the espforge checkout (used to compute the local binary path when
/// `use_local`); `debug_or_release` selects the `cargo build` profile. These
/// are read from an `answers.yaml` in the cwd (the carried config) and written
/// back into the project folder as `answers.yaml` with a `spec:` self-pointer.
///
/// Defaults (when `answers.yaml` is absent or a field is omitted):
/// - `use_local: false`        → generated code uses published crates.io deps
/// - `path: espforge`          → the local binary defaults to `espforge` on PATH
///                               (on Windows this is `espforge.exe`)
/// - `debug_or_release: debug` → `just build` runs a plain `cargo build`
#[derive(Debug, Clone)]
struct Answers {
    use_local: bool,
    path: String,
    debug_or_release: Profile,
}

impl Default for Answers {
    fn default() -> Self {
        Answers {
            use_local: false,
            path: default_binary(cfg!(windows)),
            debug_or_release: Profile::Debug,
        }
    }
}

/// Which profile `just build` compiles the generated firmware with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Profile {
    #[default]
    Debug,
    Release,
}

/// The `espforge` binary the generated `justfile` shells out to, by default
/// just `espforge` on PATH. Cross-platform aware: on Windows the on-disk
/// binary is `espforge.exe`, so we append `.exe` to a bare name/path.
fn default_binary(is_windows: bool) -> String {
    if is_windows {
        "espforge.exe".to_string()
    } else {
        "espforge".to_string()
    }
}

/// Normalise a user-supplied `path` to the binary name the host can actually
/// execute. On Windows, a path with no file extension (e.g. `/path/espforge`
/// or just `espforge`) is given the `.exe` extension, matching the binary the
/// `cargo build` produces (`target/debug/espforge.exe`). A path that already
/// has any extension (`.exe`, `.com`, a versioned `espforge-1.2.3`, ...) is
/// left verbatim, because the cargo output is always `.exe` and an explicit
/// extension reflects a deliberate choice. Non-Windows keeps the path verbatim.
///
/// Takes `is_windows` explicitly (rather than `cfg!(windows)`) so the Windows
/// branch is unit-testable on any host.
fn platform_binary(path: &str, is_windows: bool) -> String {
    if is_windows {
        let has_extension = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.contains('.'))
            .unwrap_or(false);
        if !has_extension {
            return format!("{path}.exe");
        }
    }
    path.to_string()
}

/// Compute `from` expressed as a lexical path relative to `base`, resolving
/// `..`/`..` components without touching the filesystem. Both paths are taken
/// as-is (typically both relative to the same working directory), so the result
/// points from `base` to `from` — used to re-base the espforge checkout
/// (`path:` in `answers.yaml`, relative to the project dir) to the generated
/// project's `out` directory. Returns `None` only if `from` ends up escaping
/// with no common root.
fn diff_paths(from: &std::path::Path, base: &std::path::Path) -> Option<String> {
    // Lexically normalise: drop `.`, and cancel a `..` against the preceding
    // concrete component only. A `..` that has no concrete component to cancel
    // (leading `..`, or consecutive `..`) is preserved — unlike std path
    // canonicalisation, which would error or stop at a root.
    fn norm(p: &std::path::Path) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in p.components() {
            let s = c.as_os_str().to_string_lossy().into_owned();
            match s.as_str() {
                "." => {}
                ".." => match out.last().map(|l| l.as_str()) {
                    Some("..") | None => out.push("..".to_string()),
                    Some(_) => {
                        out.pop();
                    }
                },
                other => out.push(other.to_string()),
            }
        }
        out
    }
    let from = norm(from);
    let base = norm(base);

    // Strip the common prefix.
    let mut i = 0;
    while i < from.len() && i < base.len() && from[i] == base[i] {
        i += 1;
    }
    let mut rel: Vec<String> = vec!["..".to_string(); base.len() - i];
    rel.extend_from_slice(&from[i..]);
    if rel.is_empty() {
        rel.push(".".to_string());
    }
    Some(rel.join("/"))
}

/// Read `answers.yaml` from `dir` if present. The file may be wrapped in a
/// top-level `espforge:` mapping (the carried-down shape); bare keys are also
/// accepted for convenience. Missing file or unreadable content falls back to
/// the defaults (this is an optional convenience file, not a hard requirement).
///
/// Defaults: `use_local = false` (published crates.io deps), `path` =
/// `espforge` (or `espforge.exe` on Windows), and `debug_or_release = debug`.
fn read_answers(dir: &std::path::Path) -> Answers {
    let path = dir.join("answers.yaml");
    match std::fs::read_to_string(&path) {
        Ok(text) => read_answers_from_str(&text),
        Err(_) => Answers::default(),
    }
}

/// Parse `answers.yaml` text into [`Answers`]. The file may be wrapped in a
/// top-level `espforge:` mapping (the carried-down shape) or use bare keys.
/// Unparseable content or a missing `espforge:` mapping falls back to the
/// defaults (this is an optional convenience file, not a hard requirement).
fn read_answers_from_str(text: &str) -> Answers {
    let mut a = Answers::default();
    // `use_local` is a real YAML boolean; `path` and `debug_or_release` are
    // parsed as strings (the carried-down `answers.yaml` quotes them) and
    // normalised below.
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        use_local: Option<bool>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default, rename = "debug_or_release")]
        debug_or_release: Option<String>,
    }
    // Parse as a generic mapping first: if the file nests the options under a
    // top-level `espforge:` key (the carried-down shape), read from there.
    // Otherwise fall back to bare keys at the top level. A bare-keys
    // `from_str::<Raw>` always "succeeds" (all fields optional, extra keys
    // ignored), so it must only be used when there is no `espforge:` wrapper.
    let raw: Raw = match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text) {
        Ok(value) => match value.get("espforge").and_then(|e| e.as_mapping()) {
            // `espforge:`-wrapped shape.
            Some(map) => match serde_yaml_ng::from_value::<Raw>(serde_yaml_ng::Value::Mapping(map.clone())) {
                Ok(r) => r,
                Err(_) => return a,
            },
            // No `espforge:` wrapper -> bare keys at top level.
            None => match serde_yaml_ng::from_value::<Raw>(value) {
                Ok(r) => r,
                Err(_) => return a,
            },
        },
        Err(_) => return a,
    };
    if let Some(v) = raw.use_local {
        a.use_local = v;
    }
    if let Some(v) = raw.path {
        a.path = v;
    }
    if let Some(v) = raw.debug_or_release {
        a.debug_or_release = match v.trim().to_ascii_lowercase().as_str() {
            "release" => Profile::Release,
            _ => Profile::Debug,
        };
    }
    a
}

/// Render the per-project `justfile`. It carries the `answers.yaml` values as
/// `just` variables, exporting `ESPFORGE_BINARY` so that `espforge build`
/// (invoked from a recipe) inherits it in its environment. `setup`/`create`
/// write this into the created project folder (ADR-001).
///
/// The `ESPFORGE_BINARY` line is selected by `use_local` + host platform (see
/// [`justfile_binary_lines`]): exactly one candidate is left *uncommented* and
/// the rest remain commented out. When `use_local` is true, the local-binary
/// placeholder is uncommented and replaced with the calculated path
/// (`path/target/<profile>/espforge`, since `path` already points at the `v2`
/// workspace); otherwise the PATH line (`espforge` on Linux, `espforge.exe` on
/// Windows) is active.
fn justfile_content(a: &Answers) -> String {
    let cargo_flags = cargo_profile_flags(a.debug_or_release);
    format!(
        r#"set shell := ["sh", "-c"]
set windows-shell := ["powershell", "-c"]

{lines}

build:
    {{{{ ESPFORGE_BINARY }}}} build
    cd build ; cargo build{cargo_flags}
"#,
        lines = justfile_binary_lines(a),
        cargo_flags = cargo_flags,
    )
}

/// Produce the three candidate `ESPFORGE_BINARY` declarations for the
/// `justfile`, starting all commented out and leaving exactly one uncommented:
///   1. local build of this checkout: `<path>/target/<profile>/espforge[.exe]`
///   2. on PATH (Linux):              `espforge`
///   3. on PATH (Windows):           `espforge.exe`
///
/// Selection:
///   - `use_local == true`  -> uncomment line 1, substituting the binary path
///     (derived from `path` + `debug_or_release`). Lines 2/3 stay commented.
///   - `use_local == false` -> uncomment line 2 on Linux, line 3 on Windows;
///     lines 1 stays commented.
fn justfile_binary_lines(a: &Answers) -> String {
    let local_binary = platform_binary(
        &format!("{}/target/{}/espforge", a.path.trim_end_matches('/'), a.debug_or_release.target_dir()),
        cfg!(windows),
    );
    // Line 1: local build. Active only when use_local; otherwise kept as the
    // `/path/to/espforge_binary` placeholder for the user to edit.
    let local_prefix = if a.use_local { "" } else { "#" };
    let local_value = if a.use_local {
        local_binary
    } else {
        "/path/to/espforge_binary".to_string()
    };
    // Lines 2/3: PATH installs. Active when !use_local, by platform.
    let (linux_active, windows_active) = if a.use_local {
        (false, false)
    } else {
        (true, cfg!(windows))
    };
    let linux_prefix = if linux_active { "" } else { "#" };
    let windows_prefix = if windows_active { "" } else { "#" };
    format!(
        "{local_prefix}export ESPFORGE_BINARY := \"{local_value}\"\n\
         {linux_prefix}export ESPFORGE_BINARY := \"espforge\"\n\
         {windows_prefix}export ESPFORGE_BINARY := \"espforge.exe\"",
        local_prefix = local_prefix,
        local_value = local_value,
        linux_prefix = linux_prefix,
        windows_prefix = windows_prefix,
    )
}

/// The `cargo build` profile flags for the chosen debug/release profile,
/// identical on every platform (no per-OS branching needed).
fn cargo_profile_flags(profile: Profile) -> &'static str {
    match profile {
        Profile::Release => " --release",
        Profile::Debug => "",
    }
}

/// The cargo target subdirectory a profile builds into (`debug` or `release`),
/// used to derive the local espforge binary path under `path/v2/target/...`.
impl Profile {
    fn target_dir(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Debug => "debug",
        }
    }
}

/// Write the `justfile` (derived from `answers.yaml`) into the created project
/// folder (project root, next to `answers.yaml`), so it is the obvious recipe
/// to run with `just`.
fn write_justfile(dest: &std::path::Path, a: &Answers) -> anyhow::Result<()> {
    let content = justfile_content(a);
    std::fs::write(dest.join("justfile"), content)
        .map_err(|e| anyhow::anyhow!("failed to write justfile: {e}"))?;
    Ok(())
}

/// Write the carried-down `answers.yaml` into the project root (alongside the
/// spec), wrapped in an `espforge:` mapping. `espforge build` reads this to
/// honour `use_local` when run directly rather than through `just`. Env vars
/// take precedence.
fn write_settings(dest: &std::path::Path, a: &Answers, config: &str) -> anyhow::Result<()> {
    let profile = match a.debug_or_release {
        Profile::Release => "release",
        Profile::Debug => "debug",
    };
    let text = format!(
        "espforge:\n  use_local: {}\n  path: \"{}\"\n  debug_or_release: \"{}\"\n  config: \"{}\"\n",
        a.use_local, a.path, profile, config
    );
    std::fs::write(dest.join("answers.yaml"), text)
        .map_err(|e| anyhow::anyhow!("failed to write answers.yaml: {e}"))?;
    Ok(())
}

/// Read `answers.yaml` from the project root (the directory containing the
/// spec), if present. Used by `build` to honour `use_local` when invoked
/// directly rather than through `just`.
fn read_settings(spec_dir: &std::path::Path) -> Option<Answers> {
    let path = spec_dir.join("answers.yaml");
    let text = std::fs::read_to_string(&path).ok()?;
    Some(read_answers_from_str(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_binary_linux_keeps_path_verbatim() {
        assert_eq!(platform_binary("espforge", false), "espforge");
        assert_eq!(platform_binary("/path/to/espforge", false), "/path/to/espforge");
        // An explicit .exe on a non-Windows host is left alone.
        assert_eq!(platform_binary("espforge.exe", false), "espforge.exe");
    }

    #[test]
    fn platform_binary_windows_appends_exe() {
        // Bare names and bare paths get the .exe suffix on Windows.
        assert_eq!(platform_binary("espforge", true), "espforge.exe");
        assert_eq!(platform_binary("/path/to/espforge", true), "/path/to/espforge.exe");
        // An explicit .exe is never overridden.
        assert_eq!(platform_binary("espforge.exe", true), "espforge.exe");
        // A different extension is left intact.
        assert_eq!(platform_binary("espforge.com", true), "espforge.com");
    }

    #[test]
    fn default_binary_is_platform_aware() {
        assert_eq!(default_binary(false), "espforge");
        assert_eq!(default_binary(true), "espforge.exe");
    }

    #[test]
    fn debug_or_release_maps_to_cargo_flags() {
        assert_eq!(cargo_profile_flags(Profile::Debug), "");
        assert_eq!(cargo_profile_flags(Profile::Release), " --release");
    }

    #[test]
    fn read_answers_parses_new_format() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("espforge_ans_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut f = std::fs::File::create(dir.join("answers.yaml")).unwrap();
        // The carried-down shape: options wrapped under `espforge:`, quoted.
        write!(
            f,
            "espforge:\n  use_local: true\n  path: /path/to/espforge\n  debug_or_release: \"release\"\n"
        )
        .unwrap();
        drop(f);

        let a = read_answers(&dir);
        assert!(a.use_local);
        assert_eq!(a.path, "/path/to/espforge");
        assert_eq!(a.debug_or_release, Profile::Release);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_answers_defaults_when_missing() {
        let dir = std::env::temp_dir().join(format!("espforge_ans_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = read_answers(&dir);
        assert!(!a.use_local);
        assert_eq!(a.path, default_binary(cfg!(windows)));
        assert_eq!(a.debug_or_release, Profile::Debug);
    }

    #[test]
    fn justfile_local_uses_calculated_binary() {
        let a = Answers {
            use_local: true,
            path: "/path/to/espforge".to_string(),
            debug_or_release: Profile::Release,
        };
        let jf = justfile_content(&a);
        // Local build: uncommented, with calculated path (no extra /v2 since
        // `path` already points at the v2 workspace).
        assert!(jf.contains("export ESPFORGE_BINARY := \"/path/to/espforge/target/release/espforge\""));
        // PATH candidates stay commented.
        assert!(jf.contains("#export ESPFORGE_BINARY := \"espforge\""));
        assert!(jf.contains("cargo build --release"));
    }

    #[test]
    fn justfile_nonlocal_uncomments_path_binary() {
        let a = Answers {
            use_local: false,
            path: "/path/to/espforge".to_string(),
            debug_or_release: Profile::Debug,
        };
        let jf = justfile_content(&a);
        // Local placeholder stays commented.
        assert!(jf.contains("#export ESPFORGE_BINARY := \"/path/to/espforge_binary\""));
        // On Linux, the `espforge` PATH line is active; `espforge.exe` commented.
        if cfg!(windows) {
            assert!(jf.contains("export ESPFORGE_BINARY := \"espforge.exe\""));
            assert!(jf.contains("#export ESPFORGE_BINARY := \"espforge\""));
        } else {
            assert!(jf.contains("export ESPFORGE_BINARY := \"espforge\""));
            assert!(jf.contains("#export ESPFORGE_BINARY := \"espforge.exe\""));
        }
    }

    #[test]
    fn diff_paths_rebases_checkout_to_out() {
        use std::path::Path;
        // `path:` is relative to the project dir; the manifest is written into
        // `out` (one level deeper), so the dep must gain an extra `../`.
        let from = Path::new("blink/../../mohankumargupta/espforge/v2");
        let base = Path::new("blink/build");
        assert_eq!(
            diff_paths(from, base),
            Some("../../../mohankumargupta/espforge/v2".to_string())
        );
        // Sibling under a shared parent needs a `..`.
        assert_eq!(
            diff_paths(Path::new("a/b/c"), Path::new("a/b/d")),
            Some("../c".to_string())
        );
        // Identical paths resolve to ".".
        assert_eq!(
            diff_paths(Path::new("a/b/c"), Path::new("a/b/c")),
            Some(".".to_string())
        );
    }

    // --- Conditional compilation (design §19) ---------------------------------
    // A generated firmware project must compile only the `espforge-runtime`
    // modules (and external crates) it actually uses. We assert on the emitted
    // `Cargo.toml` manifest text — the unit under test for manifest policy.
    // These tests run the published-version path (ESPFORGE_USE_LOCAL unset).

    /// Run the full pipeline (parse -> validate -> resolve -> emit) on an
    /// example spec and return the emitted `Cargo.toml` text.
    fn emitted_cargo_toml(spec_rel: &str) -> String {
        // Resolve the example path relative to the workspace `v2` root.
        let v2 = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let spec = std::path::Path::new(&v2)
            .join("../espforge-examples/examples")
            .join(spec_rel);
        let project = espforge::parse::parse_file(&spec)
            .unwrap_or_else(|e| panic!("parse {}: {e:?}", spec.display()));
        espforge::pipeline::validate(&project).expect("validate");
        let ir = espforge::pipeline::resolve(&project);
        // Option B (ADR-012): `emit` merges into an esp-generate base Cargo.toml
        // in `out_dir`. Seed a temp dir with a minimal base so the unit path
        // produces a valid manifest.
        let out = std::env::temp_dir().join(format!(
            "espforge_main_{}_{}",
            std::process::id(),
            spec_rel.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>()
        ));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("Cargo.toml"),
            "[package]\nname = \"espforge_project\"\nversion = \"0.1.0\"\n\n[dependencies]\n",
        )
        .unwrap();
        let artifacts = espforge::emit::rust::emit(&ir, &out).expect("emit");
        artifacts
            .into_iter()
            .find(|a| a.path == "Cargo.toml")
            .map(|a| a.content)
            .expect("Cargo.toml artifact")
    }

    #[test]
    fn helloworld_uses_no_optional_runtime_features() {
        let cargo = emitted_cargo_toml("01.Basics/helloworld/helloworld.yaml");
        // No runtime features => bare dep, no `features =` clause at all.
        assert!(
            cargo.contains("espforge-runtime = \"0.1.0\""),
            "expected bare espforge-runtime dep, got:\n{cargo}"
        );
        assert!(
            !cargo.contains("features ="),
            "helloworld must not request any espforge-runtime features:\n{cargo}"
        );
        // No driver-only external crates should appear.
        assert!(
            !cargo.contains("ssd1306 = "),
            "helloworld must not depend on the ssd1306 driver crate:\n{cargo}"
        );
        assert!(
            !cargo.contains("esp-wifi = "),
            "helloworld must not depend on esp-wifi:\n{cargo}"
        );
    }

    #[test]
    fn display_requests_ssd1306_runtime_feature() {
        let cargo = emitted_cargo_toml("06.Displays/display/display.yaml");
        assert!(
            cargo.contains("espforge-runtime = { version = \"0.1.0\", features = [\"ssd1306\"] }")
                || cargo.contains("features = [ \"ssd1306\" ]"),
            "display must require the ssd1306 runtime feature:\n{cargo}"
        );
        // The `i2c` component is also used by the display device, so it must be
        // present too (feature union is deduped + sorted).
        assert!(
            cargo.contains("\"i2c\"") || cargo.contains("i2c"),
            "display uses an i2c bus; i2c feature must be present:\n{cargo}"
        );
    }
}

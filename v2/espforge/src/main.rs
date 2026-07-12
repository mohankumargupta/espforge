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
            if std::env::var("ESPFORGE_USE_LOCAL").is_err() {
                if let Some(settings) =
                    read_settings(project.parent().unwrap_or_else(|| std::path::Path::new(".")))
                {
                    if settings.use_local {
                        // SAFETY: single-threaded CLI startup; set once before emit.
                        unsafe { std::env::set_var("ESPFORGE_USE_LOCAL", "true") };
                    }
                }
            }
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
    // `use_local` when run directly rather than through `just`.
    write_settings(&dest, &answers)?;
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

/// Copy `<example>/app/rust/app.rs` to `<dest>/src/app.rs` (user-owned).
fn copy_app(example: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let app = espforge_examples::asset(example, "app/rust/app.rs")
        .ok_or_else(|| anyhow::anyhow!("template `{example}` is missing app/rust/app.rs"))?;
    let dest_path = dest.join("src").join("app.rs");
    std::fs::write(dest_path, app)
        .map_err(|e| anyhow::anyhow!("failed to write src/app.rs: {e}"))?;
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
    // Fields are parsed as strings (the carried-down `answers.yaml` quotes
    // them, e.g. `use_local: "false"`) and normalised below, so both bare and
    // quoted forms are accepted.
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        use_local: Option<String>,
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
        a.use_local = matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes");
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
/// `just` variables prefixed `ESPFORGE_`, exporting them so that `espforge
/// build` (invoked from a recipe) inherits them in its environment. `setup`/
/// `create` write this into the created project folder (ADR-001).
///
/// The `ESPFORGE_BINARY` line is selected by `use_local` + host platform (see
/// [`justfile_binary_lines`]): exactly one candidate is left *uncommented* and
/// the rest are commented out. If `use_local`, the local binary path is derived
/// from `path` (the espforge checkout) and substituted in; otherwise `espforge`
/// on PATH (or `espforge.exe` on Windows) is selected.
fn justfile_content(a: &Answers) -> String {
    let use_local = if a.use_local { "true" } else { "false" };
    let cargo_flags = cargo_profile_flags(a.debug_or_release);
    format!(
        r#"set shell := ["sh", "-c"]
set windows-shell := ["powershell", "-c"]

# When true, generated projects use local espforge crates (built from this
# checkout) instead of the published crates.io versions.
export ESPFORGE_USE_LOCAL := "{use_local}"
{lines}

build:
    {{{{ ESPFORGE_BINARY }}}} build
    cd build ; cargo build{cargo_flags}
"#,
        use_local = use_local,
        lines = justfile_binary_lines(a),
        cargo_flags = cargo_flags,
    )
}

/// Produce the commented-out candidate `ESPFORGE_BINARY` declarations for the
/// `justfile`, with exactly one left uncommented based on `use_local` and the
/// host platform. Candidate set (order matters only for readability):
///   - local build (this checkout): `<checkout>/v2/target/<profile>/espforge[.exe]`
///   - local install on PATH:        `espforge` (or `espforge.exe` on Windows)
///
/// When `use_local` is true the **local-build** line is uncommented and the
/// PATH line is commented out; otherwise the PATH line is active and the
/// local-build line is commented out. The binary path runs through
/// [`platform_binary`] so it is correct on Windows.
fn justfile_binary_lines(a: &Answers) -> String {
    // `path` may point at the repo root (`../espforge`) or the `v2` workspace
    // (`../espforge/v2`). Strip a trailing `/v2` so the `v2/target/...` suffix
    // is always correct either way.
    let checkout = a.path.trim_end_matches('/').trim_end_matches("/v2");
    let local = platform_binary(checkout, cfg!(windows));
    let local_binary = format!("{}/v2/target/{}/espforge", local, a.debug_or_release.target_dir());
    let path_binary = platform_binary("espforge", cfg!(windows));
    let (local_active, path_active) = if a.use_local {
        (true, false)
    } else {
        (false, true)
    };
    let local_line = format!(
        "{}export ESPFORGE_BINARY := \"{}\"",
        if local_active { "" } else { "# " },
        local_binary
    );
    let path_line = format!(
        "{}export ESPFORGE_BINARY := \"{}\"",
        if path_active { "" } else { "# " },
        path_binary
    );
    format!(
        "# Local build of this checkout (uncomment / edit to taste):\n{local_line}\n# On PATH (release installed via `cargo install`):\n{path_line}"
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
fn write_settings(dest: &std::path::Path, a: &Answers) -> anyhow::Result<()> {
    let use_local = if a.use_local { "true" } else { "false" };
    let profile = match a.debug_or_release {
        Profile::Release => "release",
        Profile::Debug => "debug",
    };
    let text = format!(
        "espforge:\n  use_local: \"{}\"\n  path: \"{}\"\n  debug_or_release: \"{}\"\n",
        use_local, a.path, profile
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
            "espforge:\n  use_local: \"true\"\n  path: /path/to/espforge\n  debug_or_release: \"release\"\n"
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
    fn justfile_uses_platform_binary_and_profile() {
        let a = Answers {
            use_local: true,
            path: "/path/to/espforge".to_string(),
            debug_or_release: Profile::Release,
        };
        let jf = justfile_content(&a);
        assert!(jf.contains("export ESPFORGE_BINARY := \"/path/to/espforge/v2/target/release/espforge\""));
        assert!(jf.contains("cargo build --release"));
    }
}

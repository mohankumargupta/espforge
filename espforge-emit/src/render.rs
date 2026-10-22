//! Renders the embedded project template (`espforge-espgenerate-template`)
//! with minijinja, deciding per file whether it is emitted, where it goes,
//! and who owns it.
//!
//! * Every file in the template tree is registered by its path, so
//!   `{% include ".template/partials/main_blocking.rs" %}` resolves.
//! * `metadata.toml` is not a template; it controls emission (see
//!   [`parse_metadata`]). Files under `.template/` are partials: includable
//!   but never emitted themselves.
//! * Example overlays (`render_project_with_overlay`) layer an example's own
//!   Wokwi files over the template's.
//! * A file whose name ends in `.tmpl` is emitted without that suffix
//!   (`Cargo.toml.tmpl` -> `Cargo.toml`; the suffix keeps cargo from treating
//!   the template directory as a nested package).

use anyhow::{Context as _, Result, anyhow, bail};
use espforge_espgenerate_template::TEMPLATE_DIR;
use espforge_model::{Artifact, Asset, Ownership};
use include_dir::Dir;
use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value};
use toml_edit::DocumentMut;

const METADATA_FILE: &str = "metadata.toml";
const PARTIALS_PREFIX: &str = ".template/";

// ------------------------------------------------------------- template files

/// Every embedded file as (`/`-separated path, bytes), paths composed level by
/// level so they do not depend on include_dir's prefix choices.
fn collect_files(dir: &Dir<'static>, prefix: &str, out: &mut Vec<(String, &'static [u8])>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    };
    for f in dir.files() {
        let name = f
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        out.push((join(name), f.contents()));
    }
    for d in dir.dirs() {
        let name = d
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        collect_files(d, &join(name), out);
    }
}

fn all_files() -> Vec<(String, &'static [u8])> {
    let mut out = Vec::new();
    collect_files(&TEMPLATE_DIR, "", &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Paths of the files that can be emitted (excludes `metadata.toml` and the
/// `.template/` partials), sorted.
pub fn template_paths() -> Vec<String> {
    all_files()
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p != METADATA_FILE && !p.starts_with(PARTIALS_PREFIX))
        .collect()
}

/// The minijinja environment: strict about undefined names, no stray blank
/// lines around `{% %}` blocks, no escaping (these are not HTML), and every
/// template file (including partials) registered by path.
pub fn environment() -> Result<Environment<'static>> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_keep_trailing_newline(true);
    // The default callback would JSON-escape values in `*.json` templates.
    env.set_auto_escape_callback(|_| AutoEscape::None);

    for (path, bytes) in all_files() {
        if path == METADATA_FILE {
            continue;
        }
        let text = std::str::from_utf8(bytes)
            .with_context(|| format!("template `{path}` is not valid UTF-8"))?
            .to_string();
        env.add_template_owned(path.clone(), text)
            .map_err(|e| anyhow!("template `{path}`: {e:#}"))?;
    }
    Ok(env)
}

pub fn render_template(env: &Environment, name: &str, ctx: &Value) -> Result<String> {
    env.get_template(name)
        .and_then(|t| t.render(ctx))
        .map_err(|e| anyhow!("rendering `{name}`: {e:#}"))
}

// ------------------------------------------------------------------ metadata

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitEntry {
    /// Output path (without `.tmpl`).
    pub path: String,
    /// minijinja expression; the file is emitted only if it is true.
    pub when: Option<String>,
    /// minijinja template for a different output path.
    pub rename: Option<String>,
    /// Ownership for this file, overriding the glob rules.
    pub own: Option<Ownership>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub emit: Vec<EmitEntry>,
    /// (glob, ownership); first match wins; default is `Owned`.
    pub ownership: Vec<(String, Ownership)>,
}

fn parse_own(s: &str) -> Result<Ownership> {
    match s {
        "owned" => Ok(Ownership::Owned),
        "seed-once" => Ok(Ownership::SeedOnce),
        "not-owned" => Ok(Ownership::NotOwned),
        other => bail!("unknown ownership \"{other}\" (expected owned, seed-once or not-owned)"),
    }
}

/// Globs: an exact path, or `dir/**` for everything below `dir`.
fn check_glob(g: &str) -> Result<()> {
    if g.contains('*') && !(g.ends_with("/**") && !g[..g.len() - 3].contains('*')) {
        bail!("unsupported glob \"{g}\" (use an exact path or `dir/**`)");
    }
    Ok(())
}

pub fn glob_match(glob: &str, path: &str) -> bool {
    match glob.strip_suffix("/**") {
        Some(dir) => path.starts_with(&format!("{dir}/")),
        None => glob == path,
    }
}

/// Parse `metadata.toml`:
///
/// ```toml
/// emit = [
///   { path = "diagram.json", when = "opt.wokwi" },
///   { path = ".vscode/tasks.json", when = "opt.vscode", own = "seed-once" },
/// ]
/// ownership = [
///   { glob = "src/**", own = "owned" },
///   { glob = ".cargo/**", own = "seed-once" },
/// ]
/// ```
///
/// Files not listed under `emit` are always emitted.
pub fn parse_metadata(src: &str) -> Result<Metadata> {
    let doc: DocumentMut = src.parse().context("invalid metadata.toml")?;
    let mut md = Metadata::default();

    if let Some(arr) = doc.get("emit").and_then(|i| i.as_array()) {
        for item in arr.iter() {
            let t = item
                .as_inline_table()
                .context("each `emit` entry must be an inline table")?;
            let get = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
            md.emit.push(EmitEntry {
                path: get("path").context("an `emit` entry is missing `path`")?,
                when: get("when"),
                rename: get("as"),
                own: get("own").map(|s| parse_own(&s)).transpose()?,
            });
        }
    }
    if let Some(arr) = doc.get("ownership").and_then(|i| i.as_array()) {
        for item in arr.iter() {
            let t = item
                .as_inline_table()
                .context("each `ownership` entry must be an inline table")?;
            let glob = t
                .get("glob")
                .and_then(|v| v.as_str())
                .context("an `ownership` entry is missing `glob`")?;
            let own = t
                .get("own")
                .and_then(|v| v.as_str())
                .context("an `ownership` entry is missing `own`")?;
            check_glob(glob)?;
            md.ownership.push((glob.to_string(), parse_own(own)?));
        }
    }
    Ok(md)
}

pub fn metadata() -> Result<Metadata> {
    let file = TEMPLATE_DIR
        .get_file(METADATA_FILE)
        .context("the template has no metadata.toml")?;
    parse_metadata(file.contents_utf8().context("metadata.toml is not UTF-8")?)
}

// ---------------------------------------------------------------------- plan

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Template path to render.
    pub template: String,
    /// Output path (relative to the project directory).
    pub out_path: String,
    pub ownership: Ownership,
}

/// Decide which template files are emitted for this context, under which
/// names, and with what ownership. `when` conditions are evaluated only for
/// files that actually exist in the template.
pub fn plan(env: &Environment, md: &Metadata, ctx: &Value) -> Result<Vec<Planned>> {
    let mut out = Vec::new();
    for template in template_paths() {
        let default_out = template
            .strip_suffix(".tmpl")
            .unwrap_or(&template)
            .to_string();
        let entry = md.emit.iter().find(|e| e.path == default_out);
        let mut out_path = default_out;

        if let Some(e) = entry {
            if let Some(when) = &e.when {
                let on = env
                    .compile_expression(when)
                    .and_then(|x| x.eval(ctx))
                    .map_err(|err| anyhow!("condition `{when}` for `{}`: {err:#}", e.path))?;
                if !on.is_true() {
                    continue;
                }
            }
            if let Some(rename) = &e.rename {
                out_path = env
                    .render_str(rename, ctx)
                    .map_err(|err| anyhow!("`as` for `{}`: {err:#}", e.path))?;
            }
        }

        let ownership = entry
            .and_then(|e| e.own)
            .or_else(|| {
                md.ownership
                    .iter()
                    .find(|(g, _)| glob_match(g, &out_path))
                    .map(|(_, o)| *o)
            })
            .unwrap_or(Ownership::Owned);

        out.push(Planned {
            template,
            out_path,
            ownership,
        });
    }
    Ok(out)
}

/// Render every file the plan selects.
pub fn render_project(ctx: &Value) -> Result<Vec<Artifact>> {
    let env = environment()?;
    let md = metadata()?;
    plan(&env, &md, ctx)?
        .into_iter()
        .map(|p| {
            let content = render_template(&env, &p.template, ctx)?;
            Ok(Artifact {
                path: p.out_path,
                content,
                ownership: p.ownership,
            })
        })
        .collect()
}

// ------------------------------------------------------------------- overlay

/// One file of an example, with its path relative to the example directory.
#[derive(Debug, Clone, Copy)]
pub struct OverlayFile<'a> {
    pub path: &'a str,
    pub contents: &'a [u8],
}

/// Everything a render produces: text files and binary assets.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub artifacts: Vec<Artifact>,
    pub assets: Vec<Asset>,
}

fn opt_flag(ctx: &Value, name: &str) -> Result<bool> {
    ctx.get_attr("opt")
        .and_then(|o| o.get_attr(name))
        .map(|v| v.is_true())
        .map_err(|e| anyhow!("reading option `{name}` from the context: {e:#}"))
}

/// Render the template, then layer an example's files over it:
///
/// * `diagram.json` / `wokwi.toml` replace the template's and are rendered
///   through the same engine (so `{{ board }}` etc. resolve). They only
///   apply when the `wokwi` option emitted the template's version.
/// * `chip.json` and `chip.wasm` are always copied (when `wokwi` is on);
///   the `chip/` source directory only when `keep-zig-source` is on. An
///   example without `chip/` makes that a no-op.
///   These are seed-once, so rebuilding a chip locally is never clobbered.
///   A seeded chip also appends a `[[chip]]` section (see [`crate::wokwi`])
///   to the emitted `wokwi.toml`, whichever one was emitted: the template's
///   or the example's own.
/// * Everything else (`example.espf.txt`, `app/`, ...) is the CLI's business
///   and ignored here.
pub fn render_project_with_overlay(ctx: &Value, overlay: &[OverlayFile]) -> Result<Rendered> {
    let env = environment()?;
    let md = metadata()?;
    let mut artifacts = plan(&env, &md, ctx)?
        .into_iter()
        .map(|p| {
            let content = render_template(&env, &p.template, ctx)?;
            Ok(Artifact {
                path: p.out_path,
                content,
                ownership: p.ownership,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut assets = Vec::new();

    let wokwi = opt_flag(ctx, "wokwi")?;
    let keep_zig = opt_flag(ctx, "keep-zig-source")?;

    let mut chip_json: Option<&[u8]> = None;
    let mut have_chip = false;
    for f in overlay {
        match f.path {
            "diagram.json" | "wokwi.toml" => {
                if let Some(a) = artifacts.iter_mut().find(|a| a.path == f.path) {
                    let text = std::str::from_utf8(f.contents)
                        .with_context(|| format!("example `{}` is not valid UTF-8", f.path))?;
                    a.content = env
                        .render_str(text, ctx)
                        .map_err(|e| anyhow!("example `{}`: {e:#}", f.path))?;
                }
            }
            "chip.json" | "chip.wasm" if wokwi => {
                have_chip = true;
                if f.path == "chip.json" {
                    chip_json = Some(f.contents);
                }
                seed(f, &mut artifacts, &mut assets);
            }
            p if p.starts_with("chip/") && wokwi && keep_zig => {
                seed(f, &mut artifacts, &mut assets)
            }
            _ => {}
        }
    }

    // A seeded custom chip needs a `[[chip]]` section in the emitted
    // `wokwi.toml` so the `chip-<name>` part in diagram.json resolves.
    if have_chip {
        if let Some(a) = artifacts.iter_mut().find(|a| a.path == "wokwi.toml") {
            let json = chip_json.and_then(|b| std::str::from_utf8(b).ok());
            a.content.push_str(&crate::wokwi::chip_section(json));
        }
    }

    Ok(Rendered { artifacts, assets })
}

/// Copy an overlay file as seed-once: text becomes an artifact, binary
/// (and anything `.wasm`) an asset.
fn seed(f: &OverlayFile, artifacts: &mut Vec<Artifact>, assets: &mut Vec<Asset>) {
    match std::str::from_utf8(f.contents) {
        Ok(text) if !f.path.ends_with(".wasm") => {
            artifacts.push(Artifact::seed_once(f.path, text));
        }
        _ => assets.push(Asset::seed_once(f.path, f.contents)),
    }
}

/// `README.txt`: says what is machine-owned and what is yours.
pub fn readme(project_name: &str, artifacts: &[Artifact], assets: &[Asset]) -> Artifact {
    let mut owned: Vec<&str> = artifacts
        .iter()
        .filter(|a| a.ownership == Ownership::Owned)
        .map(|a| a.path.as_str())
        .collect();
    let mut seeded: Vec<&str> = artifacts
        .iter()
        .filter(|a| a.ownership == Ownership::SeedOnce)
        .map(|a| a.path.as_str())
        .chain(
            assets
                .iter()
                .filter(|a| a.ownership == Ownership::SeedOnce)
                .map(|a| a.path.as_str()),
        )
        .collect();
    owned.push("README.txt");
    owned.sort_unstable();
    seeded.sort_unstable();
    let list = |v: &[&str]| {
        if v.is_empty() {
            "  (none)".to_string()
        } else {
            v.iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    Artifact::owned(
        "README.txt",
        format!(
            "espforge generated project: {project_name}\n\
==========================================\n\
\n\
SOURCE OF TRUTH\n\
  This directory is GENERATED from an .espf file. Edit the .espf (and your\n\
  src/app.rs next to it), then re-run `espforge build`.\n\
\n\
MACHINE-OWNED (do not edit; regenerated every build):\n\
{}\n\
\n\
SEEDED ONCE (written the first time, then yours; never overwritten):\n\
{}\n\
\n\
REGENERATE:\n\
  espforge build <your.espf> --out <this dir>\n",
            list(&owned),
            list(&seeded)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::RuntimeSource;
    use crate::context::build_context;
    use espforge_model::{Console, Meta, Runtime};
    use espforge_options::{Answers, DerivedInputs};
    use toml_edit::DocumentMut;

    struct Case {
        runtime: Runtime,
        chip: &'static str,
        defaults: bool,
        flags: Vec<(&'static str, &'static str)>,
        services: Vec<&'static str>,
    }

    fn case() -> Case {
        Case {
            runtime: Runtime::Blocking,
            chip: "esp32c3",
            defaults: true,
            flags: vec![],
            services: vec![],
        }
    }

    fn ctx(c: Case) -> Value {
        let meta = Meta {
            name: "hello world".into(),
            description: None,
            chip: c.chip.into(),
            runtime: c.runtime,
            console: Console::Uart,
        };
        let mut answers = Answers::empty(0..0);
        answers.defaults = c.defaults;
        for (k, v) in &c.flags {
            answers
                .apply_flag(espforge_options::embedded_schema(), k, v)
                .unwrap();
        }
        let derived = DerivedInputs {
            services: c.services.iter().map(|s| (s.to_string(), 0..1)).collect(),
            console_span: None,
        };
        let opts = espforge_options::resolve(&meta, &answers, &derived).unwrap();
        build_context(&meta, &opts, &RuntimeSource::Published).unwrap()
    }

    fn hello() -> Value {
        ctx(case())
    }

    fn render(name: &str, ctx: &Value) -> String {
        render_template(&environment().unwrap(), name, ctx).unwrap()
    }

    fn file<'a>(r: &'a [Artifact], path: &str) -> &'a Artifact {
        r.iter()
            .find(|a| a.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
    }

    fn features(doc: &DocumentMut, dep: &str) -> Vec<String> {
        doc["dependencies"][dep]["features"]
            .as_array()
            .unwrap_or_else(|| panic!("{dep} has no features"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    // ---- 3.1 / 3.3 ---------------------------------------------------------

    #[test]
    fn environment_registers_the_partials_and_is_strict() {
        let env = environment().unwrap();
        assert!(
            env.get_template(".template/partials/main_blocking.rs")
                .is_ok()
        );
        assert!(env.render_str("{{ nope }}", hello()).is_err());
    }

    #[test]
    fn blocking_main_includes_its_partial() {
        let out = render("src/bin/main.rs", &hello());
        assert!(
            out.contains(
                "use hello_world::{app, Components, Context, Devices, PeripheralRegistry, CTX};"
            ),
            "{out}"
        );
        assert!(out.contains("#[esp_hal::main]"));
        assert!(!out.contains("{{") && !out.contains("{%"));
        assert!(!out.contains("\n\n\n"), "stray blank lines:\n{out}");
    }

    #[test]
    fn embassy_main_includes_the_async_partial() {
        let out = render(
            "src/bin/main.rs",
            &ctx(Case {
                runtime: Runtime::Embassy,
                ..case()
            }),
        );
        assert!(out.contains("embassy_executor::Spawner"), "{out}");
        assert!(!out.contains("{{") && !out.contains("{%"));
        assert!(!out.contains("\n\n\n"), "stray blank lines:\n{out}");
    }

    #[test]
    fn diagram_json_is_valid_json_with_board_and_serial_monitor() {
        let out = render("diagram.json", &hello());
        let v: serde_json::Value =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert_eq!(v["parts"][0]["type"], "board-esp32-c3-devkitm-1");
        assert_eq!(v["connections"][0][1], "$serialMonitor:RX");
    }

    #[test]
    fn metadata_toml_parses() {
        let md = metadata().unwrap();
        assert!(md.emit.iter().any(|e| e.path == "diagram.json"));
        assert!(
            md.ownership
                .iter()
                .any(|(g, o)| g == ".cargo/**" && *o == Ownership::SeedOnce)
        );
    }

    #[test]
    fn plan_applies_conditions_names_and_ownership() {
        let env = environment().unwrap();
        let md = metadata().unwrap();

        let on = plan(&env, &md, &hello()).unwrap();
        let find = |p: &[Planned], path: &str| p.iter().find(|x| x.out_path == path).cloned();
        let cargo = find(&on, "Cargo.toml").expect("Cargo.toml.tmpl is emitted as Cargo.toml");
        assert_eq!(cargo.template, "Cargo.toml.tmpl");
        assert_eq!(cargo.ownership, Ownership::Owned);
        assert_eq!(find(&on, "src/lib.rs").unwrap().ownership, Ownership::Owned);
        assert_eq!(
            find(&on, ".cargo/config.toml").unwrap().ownership,
            Ownership::SeedOnce
        );
        assert_eq!(
            find(&on, "rust-toolchain.toml").unwrap().ownership,
            Ownership::SeedOnce
        );
        assert_eq!(
            find(&on, "diagram.json").unwrap().ownership,
            Ownership::Owned
        );
        assert!(find(&on, "metadata.toml").is_none());
        assert!(on.iter().all(|p| !p.out_path.starts_with(".template/")));

        let off = plan(
            &env,
            &md,
            &ctx(Case {
                defaults: false,
                ..case()
            }),
        )
        .unwrap();
        assert!(find(&off, "diagram.json").is_none());
        assert!(find(&off, "wokwi.toml").is_none());
        assert!(find(&off, "src/lib.rs").is_some());
    }

    #[test]
    fn metadata_parser_handles_rename_own_and_rejects_bad_input() {
        let md = parse_metadata(
            r#"
emit = [
  { path = "a.txt", when = "opt.x", as = "{{ project_name }}.txt", own = "seed-once" },
]
ownership = [ { glob = "src/**", own = "owned" } ]
"#,
        )
        .unwrap();
        assert_eq!(md.emit[0].rename.as_deref(), Some("{{ project_name }}.txt"));
        assert_eq!(md.emit[0].own, Some(Ownership::SeedOnce));

        assert!(parse_metadata("emit = [ { when = \"x\" } ]").is_err());
        assert!(parse_metadata("emit = [ { path = \"a\", own = \"mine\" } ]").is_err());
        assert!(
            parse_metadata("ownership = [ { glob = \"src/*.rs\", own = \"owned\" } ]").is_err()
        );
    }

    #[test]
    fn globs() {
        assert!(glob_match("src/**", "src/bin/main.rs"));
        assert!(!glob_match("src/**", "src"));
        assert!(!glob_match("src/**", "srcs/x"));
        assert!(glob_match("Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("Cargo.toml", "x/Cargo.toml"));
    }

    #[test]
    fn rename_is_rendered_through_the_engine() {
        let env = environment().unwrap();
        let md = Metadata {
            emit: vec![EmitEntry {
                path: "wokwi.toml".into(),
                when: None,
                rename: Some("{{ project_name }}.wokwi.toml".into()),
                own: None,
            }],
            ownership: vec![],
        };
        let p = plan(&env, &md, &hello()).unwrap();
        assert!(p.iter().any(|x| x.out_path == "hello_world.wokwi.toml"));
    }

    // ---- 4.1 / 4.2: the whole template renders ----------------------------

    #[test]
    fn every_file_renders_for_riscv_blocking_without_stray_blank_lines() {
        let arts = render_project(&hello()).expect("whole template must render in strict mode");
        for a in &arts {
            assert!(!a.content.contains("defmt"), "defmt left in {}", a.path);
            assert!(
                !a.content.contains("{{") && !a.content.contains("{%"),
                "unrendered jinja in {}",
                a.path
            );
            assert!(
                !a.content.contains("\n\n\n"),
                "stray blank lines in {}:\n{}",
                a.path,
                a.content
            );
        }
        let paths: Vec<&str> = arts.iter().map(|a| a.path.as_str()).collect();
        for p in [
            "Cargo.toml",
            "build.rs",
            "src/lib.rs",
            "src/bin/main.rs",
            ".cargo/config.toml",
            ".cargo/esp-config.toml",
            "rust-toolchain.toml",
            "wokwi.toml",
            "diagram.json",
        ] {
            assert!(paths.contains(&p), "missing {p} in {paths:?}");
        }
    }

    #[test]
    fn blocking_cargo_toml_has_the_merged_dependencies() {
        let arts = render_project(&hello()).unwrap();
        let toml = &file(&arts, "Cargo.toml").content;
        let doc: DocumentMut = toml.parse().unwrap_or_else(|e| panic!("{e}\n{toml}"));
        assert_eq!(doc["package"]["name"].as_str(), Some("hello_world"));
        assert_eq!(doc["bin"][0]["name"].as_str(), Some("hello_world"));
        let deps = &doc["dependencies"];
        // runtime dep without the embassy feature
        assert_eq!(deps["espforge-runtime"]["version"].as_str(), Some("0.1.0"));
        assert!(deps["espforge-runtime"].get("features").is_none());
        // always present
        for d in [
            "static_cell",
            "log",
            "esp-hal",
            "esp-backtrace",
            "esp-bootloader-esp-idf",
            "critical-section",
        ] {
            assert!(deps.get(d).is_some(), "missing {d}");
        }
        // alloc is on by default
        assert!(deps.get("esp-alloc").is_some());
        // esp-println: default features off, chip + console + log
        assert_eq!(
            deps["esp-println"]["default-features"].as_bool(),
            Some(false)
        );
        let f = features(&doc, "esp-println");
        assert!(
            f.contains(&"esp32c3".into())
                && f.contains(&"uart".into())
                && f.contains(&"log-04".into())
        );
        assert!(features(&doc, "esp-hal").contains(&"unstable".into()));
        // nothing embassy / radio
        for d in [
            "esp-rtos",
            "embassy-executor",
            "embassy-time",
            "embassy-net",
            "smoltcp",
            "esp-radio",
        ] {
            assert!(deps.get(d).is_none(), "{d} should be absent");
        }
    }

    #[test]
    fn embassy_wifi_cargo_toml_has_the_async_and_radio_dependencies() {
        let c = ctx(Case {
            runtime: Runtime::Embassy,
            services: vec!["wifi"],
            ..case()
        });
        let arts = render_project(&c).expect("embassy + wifi must render");
        let toml = &file(&arts, "Cargo.toml").content;
        let doc: DocumentMut = toml.parse().unwrap_or_else(|e| panic!("{e}\n{toml}"));
        let deps = &doc["dependencies"];
        assert!(features(&doc, "espforge-runtime").contains(&"embassy".into()));
        for d in [
            "esp-rtos",
            "embassy-executor",
            "embassy-time",
            "embassy-net",
            "embedded-io-async",
            "smoltcp",
            "esp-radio",
            "esp-alloc",
            "static_cell",
        ] {
            assert!(deps.get(d).is_some(), "missing {d}");
        }
        let rtos = features(&doc, "esp-rtos");
        assert!(
            rtos.contains(&"embassy".into())
                && rtos.contains(&"esp-radio".into())
                && rtos.contains(&"esp-alloc".into())
        );
        assert!(features(&doc, "esp-radio").contains(&"wifi".into()));
        assert!(!toml.contains("\n\n\n"));
    }

    #[test]
    fn embassy_without_radio_has_executor_but_no_radio() {
        let c = ctx(Case {
            runtime: Runtime::Embassy,
            ..case()
        });
        let arts = render_project(&c).unwrap();
        let doc: DocumentMut = file(&arts, "Cargo.toml").content.parse().unwrap();
        let deps = &doc["dependencies"];
        assert!(deps.get("esp-rtos").is_some() && deps.get("embassy-executor").is_some());
        assert!(deps.get("esp-radio").is_none() && deps.get("embassy-net").is_none());
    }

    #[test]
    fn jtag_console_and_description_reach_cargo_toml() {
        let meta = Meta {
            name: "demo".into(),
            description: Some("says \"hi\"".into()),
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::JtagSerial,
        };
        let opts =
            espforge_options::resolve(&meta, &Answers::empty(0..0), &DerivedInputs::default())
                .unwrap();
        let c = build_context(&meta, &opts, &RuntimeSource::Path("../rt".into())).unwrap();
        let arts = render_project(&c).unwrap();
        let doc: DocumentMut = file(&arts, "Cargo.toml").content.parse().unwrap();
        assert!(features(&doc, "esp-println").contains(&"jtag-serial".into()));
        assert!(!features(&doc, "esp-println").contains(&"uart".into()));
        assert_eq!(doc["package"]["description"].as_str(), Some("says \"hi\""));
        assert_eq!(
            doc["dependencies"]["espforge-runtime"]["path"].as_str(),
            Some("../rt")
        );
    }

    #[test]
    fn cargo_config_follows_probe_rs_and_chip_arch() {
        let arts = render_project(&hello()).unwrap();
        let cfg = &file(&arts, ".cargo/config.toml").content;
        let doc: DocumentMut = cfg.parse().unwrap_or_else(|e| panic!("{e}\n{cfg}"));
        assert!(cfg.contains("runner = \"espflash flash --monitor --chip esp32c3\""));
        assert!(cfg.contains("force-frame-pointers") && !cfg.contains("nostartfiles"));
        assert!(doc.get("unstable").is_some());

        let probe = ctx(Case {
            flags: vec![("probe-rs", "true")],
            ..case()
        });
        let binding = render_project(&probe).unwrap();
        let cfg = &file(&binding, ".cargo/config.toml").content;
        assert!(cfg.contains("probe-rs run --chip=esp32c3"));
    }

    #[test]
    fn xtensa_chip_renders_the_xtensa_branches() {
        let c = ctx(Case {
            chip: "esp32s3",
            ..case()
        });
        let arts = render_project(&c).expect("xtensa chip must render");
        assert!(
            file(&arts, "build.rs")
                .content
                .contains("check_xtensa_linker_available")
        );
        assert!(
            file(&arts, ".cargo/config.toml")
                .content
                .contains("nostartfiles")
        );
        assert!(
            file(&arts, "rust-toolchain.toml")
                .content
                .contains("channel = \"esp\"")
        );
        assert!(!file(&arts, "build.rs").content.contains("\n\n\n"));
        let doc: DocumentMut = file(&arts, "Cargo.toml").content.parse().unwrap();
        assert!(features(&doc, "esp-hal").contains(&"esp32s3".into()));
    }

    // ---- 4.4: overlay -------------------------------------------------------

    fn overlay_of(example: &str) -> Vec<(String, &'static [u8])> {
        espforge_examples::find(example)
            .unwrap_or_else(|| panic!("no example {example}"))
            .files
            .iter()
            .map(|f| (f.path.clone(), f.contents))
            .collect()
    }

    fn run_overlay(c: Value, files: &[(String, &'static [u8])]) -> Rendered {
        let o: Vec<OverlayFile> = files
            .iter()
            .map(|(p, c)| OverlayFile {
                path: p,
                contents: c,
            })
            .collect();
        render_project_with_overlay(&c, &o).unwrap()
    }

    #[test]
    fn example_diagram_replaces_the_template_one_and_is_rendered() {
        let r = run_overlay(hello(), &overlay_of("04.Communication/i2c_scanner"));
        let d = file(&r.artifacts, "diagram.json");
        assert_eq!(d.ownership, Ownership::Owned);
        let v: serde_json::Value =
            serde_json::from_str(&d.content).unwrap_or_else(|e| panic!("{e}\n{}", d.content));
        assert_eq!(v["parts"][0]["type"], "board-esp32-c3-devkitm-1");
        assert!(
            d.content.contains("chip-chip"),
            "the example's own part must be present"
        );
        assert!(d.content.contains("$serialMonitor:RX"));
        assert_eq!(
            r.artifacts
                .iter()
                .filter(|a| a.path == "diagram.json")
                .count(),
            1
        );
    }

    #[test]
    fn chip_json_and_wasm_are_always_copied_but_zig_source_is_optional() {
        let files = overlay_of("04.Communication/i2c_scanner");

        let r = run_overlay(hello(), &files);
        assert_eq!(
            file(&r.artifacts, "chip.json").ownership,
            Ownership::SeedOnce
        );
        let wasm = r
            .assets
            .iter()
            .find(|a| a.path == "chip.wasm")
            .expect("chip.wasm copied");
        assert!(wasm.bytes.starts_with(b"\0asm"));
        assert_eq!(wasm.ownership, Ownership::SeedOnce);
        assert!(
            r.artifacts.iter().all(|a| !a.path.starts_with("chip/")),
            "zig source must not be copied by default"
        );

        let keep = ctx(Case {
            flags: vec![("keep-zig-source", "true")],
            ..case()
        });
        let r = run_overlay(keep, &files);
        assert!(
            file(&r.artifacts, "chip/chip.zig")
                .content
                .contains("chipInit")
        );
        assert!(r.artifacts.iter().any(|a| a.path == "chip/build.zig"));
    }

    #[test]
    fn a_seeded_chip_appends_the_chip_section_to_wokwi_toml() {
        let r = run_overlay(hello(), &overlay_of("04.Communication/i2c_scanner"));
        let t = file(&r.artifacts, "wokwi.toml");
        // the template's own content is untouched
        assert!(
            t.content
                .contains("elf = \"target/riscv32imc-unknown-none-elf/debug/hello_world\""),
            "{}",
            t.content
        );
        // and the chip is wired in, named from chip.json
        assert!(t.content.contains("[[chip]]\nname = 'chip'"), "{}", t.content);
        assert!(t.content.contains("binary = 'chip.wasm'"), "{}", t.content);
    }

    #[test]
    fn no_chip_means_no_chip_section_in_wokwi_toml() {
        let r = run_overlay(hello(), &overlay_of("01.Basics/helloworld"));
        assert!(!file(&r.artifacts, "wokwi.toml").content.contains("[[chip]]"));
    }

    #[test]
    fn keep_zig_source_is_a_no_op_without_a_chip_directory() {
        let keep = ctx(Case {
            flags: vec![("keep-zig-source", "true")],
            ..case()
        });
        let files: Vec<(String, &'static [u8])> =
            vec![("diagram.json".into(), &b"{\"parts\":[\"{{ board }}\"]}"[..])];
        let r = run_overlay(keep, &files);
        assert!(r.artifacts.iter().all(|a| !a.path.starts_with("chip")));
        assert!(r.assets.is_empty());
    }

    #[test]
    fn wokwi_off_drops_the_example_wokwi_files_too() {
        let off = ctx(Case {
            defaults: false,
            ..case()
        });
        let r = run_overlay(off, &overlay_of("04.Communication/i2c_scanner"));
        assert!(
            r.artifacts
                .iter()
                .all(|a| a.path != "diagram.json" && a.path != "chip.json")
        );
        assert!(r.assets.is_empty());
    }

    #[test]
    fn non_wokwi_example_files_are_ignored() {
        let r = run_overlay(hello(), &overlay_of("01.Basics/helloworld"));
        assert!(
            r.artifacts
                .iter()
                .all(|a| a.path != "app/rust/app.rs" && a.path != "example.espf.txt")
        );
        // the template's own diagram.json is kept
        assert!(r.artifacts.iter().any(|a| a.path == "diagram.json"));
    }

    #[test]
    fn readme_lists_owned_and_seeded_files() {
        let r = run_overlay(hello(), &overlay_of("04.Communication/i2c_scanner"));
        let readme = readme("hello_world", &r.artifacts, &r.assets);
        assert_eq!(readme.path, "README.txt");
        let (owned, seeded) = readme.content.split_once("SEEDED ONCE").unwrap();
        assert!(
            owned.contains("  - Cargo.toml")
                && owned.contains("  - src/lib.rs")
                && owned.contains("  - README.txt")
        );
        assert!(!owned.contains(".cargo/config.toml"));
        assert!(
            seeded.contains("  - .cargo/config.toml")
                && seeded.contains("  - chip.wasm")
                && seeded.contains("  - rust-toolchain.toml")
        );
    }
}

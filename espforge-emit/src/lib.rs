//! Turns a resolved project into files.
//!
//! * [`context`]  - the minijinja context
//! * [`render`]   - renders the embedded template (and example overlays)
//! * [`manifest`] - ownership manifest for drift detection
//! * [`generate`] is pure (data in, files out); [`write`] and
//!   [`write_assets`] touch the filesystem.
//!
//! Host-only: never depend on `espforge-runtime` from this crate.

pub mod context;
pub mod manifest;
pub mod render;

use anyhow::{Context, Result, bail};
use espforge_model::{Artifact, Asset, Meta, Ownership};
use espforge_options::ResolvedOptions;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub use context::RuntimeSource;
pub use render::OverlayFile;

/// Everything a build writes: text files and binary assets.
#[derive(Debug, Clone, Default)]
pub struct Generated {
    pub artifacts: Vec<Artifact>,
    pub assets: Vec<Asset>,
}

/// Render the project for `core` + `resolved`, layering an example's files
/// (`overlay`, empty for a plain project) over the template. Adds `README.txt`
/// and `.espforge/manifest.json` (which records the template version).
/// Does not touch the filesystem.
pub fn generate(
    core: &Meta,
    resolved: &ResolvedOptions,
    runtime_source: &RuntimeSource,
    overlay: &[OverlayFile],
) -> Result<Generated> {
    let ctx = context::build_context(core, resolved, runtime_source)?;
    let rendered = render::render_project_with_overlay(&ctx, overlay)?;
    let mut artifacts = rendered.artifacts;
    let assets = rendered.assets;

    artifacts.push(render::readme(&core.crate_name(), &artifacts, &assets));
    let manifest = manifest::emit_manifest(&artifacts, espforge_espgenerate_template::VERSION)?;
    artifacts.push(manifest);
    Ok(Generated { artifacts, assets })
}

/// Write artifacts into `out_dir`, respecting ownership:
/// - `Owned` files are overwritten, unless the previous manifest shows the
///   file was edited since we wrote it (drift): then we refuse.
/// - `SeedOnce` files are written only if absent.
/// - `NotOwned` files are never written.
pub fn write(out_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let prev = read_prev_manifest(out_dir)?;
    for a in artifacts {
        if a.ownership == Ownership::NotOwned {
            continue;
        }
        let path = out_dir.join(&a.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match a.ownership {
            Ownership::SeedOnce => {
                if !path.exists() {
                    fs::write(&path, &a.content)?;
                }
            }
            Ownership::Owned => {
                if let Some(prev_sum) = prev.get(&a.path) {
                    if let Ok(existing) = fs::read_to_string(&path) {
                        if manifest::checksum(&existing) != *prev_sum && existing != a.content {
                            bail!(
                                "refusing to overwrite edited file `{}` (drift detected). \
                                 Restore it or delete it to regenerate.",
                                a.path
                            );
                        }
                    }
                }
                fs::write(&path, &a.content)?;
            }
            Ownership::NotOwned => {}
        }
    }
    Ok(())
}

/// Write binary assets into `out_dir`: `SeedOnce` only if absent, `Owned`
/// always overwritten (assets are not drift-tracked), `NotOwned` never.
pub fn write_assets(out_dir: &Path, assets: &[Asset]) -> Result<()> {
    for a in assets {
        let path = out_dir.join(&a.path);
        match a.ownership {
            Ownership::NotOwned => continue,
            Ownership::SeedOnce if path.exists() => continue,
            _ => {}
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &a.bytes)?;
    }
    Ok(())
}

/// Convenience: [`write`] then [`write_assets`].
pub fn write_all(out_dir: &Path, generated: &Generated) -> Result<()> {
    write(out_dir, &generated.artifacts)?;
    write_assets(out_dir, &generated.assets)
}

fn read_prev_manifest(out_dir: &Path) -> Result<BTreeMap<String, String>> {
    let p = out_dir.join(".espforge/manifest.json");
    match fs::read_to_string(&p) {
        Ok(s) => manifest::parse_manifest(&s).context("corrupt .espforge/manifest.json"),
        Err(_) => Ok(BTreeMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{Console, Runtime};
    use espforge_options::{Answers, DerivedInputs};

    fn core() -> Meta {
        Meta {
            name: "hello world".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::default(),
        }
    }

    fn generate_with(example: Option<&str>) -> Generated {
        let m = core();
        let opts = espforge_options::resolve(&m, &Answers::empty(0..0), &DerivedInputs::default())
            .unwrap();
        let files: Vec<(String, &'static [u8])> = example
            .map(|e| {
                espforge_examples::find(e)
                    .unwrap_or_else(|| panic!("no example {e}"))
                    .files
                    .iter()
                    .map(|f| (f.path.clone(), f.contents))
                    .collect()
            })
            .unwrap_or_default();
        let overlay: Vec<OverlayFile> = files
            .iter()
            .map(|(p, c)| OverlayFile {
                path: p,
                contents: c,
            })
            .collect();
        generate(&m, &opts, &RuntimeSource::Published, &overlay).unwrap()
    }

    fn find<'a>(g: &'a Generated, path: &str) -> &'a Artifact {
        g.artifacts
            .iter()
            .find(|a| a.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
    }

    #[test]
    fn generate_produces_the_complete_helloworld_project() {
        let g = generate_with(None);
        let paths: Vec<&str> = g.artifacts.iter().map(|a| a.path.as_str()).collect();
        for p in [
            "Cargo.toml",
            "build.rs",
            "src/lib.rs",
            "src/generated.rs",
            "src/bin/main.rs",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "wokwi.toml",
            "diagram.json",
            "README.txt",
            ".espforge/manifest.json",
        ] {
            assert!(paths.contains(&p), "missing {p} in {paths:?}");
        }
        assert!(
            find(&g, "Cargo.toml")
                .content
                .contains("name = \"hello_world\"")
        );
        assert!(
            find(&g, "src/bin/main.rs")
                .content
                .contains("use hello_world::")
        );
        // nothing about the old scaffolder survives
        assert!(
            g.artifacts
                .iter()
                .all(|a| !a.content.contains("esp-generate"))
        );
    }

    #[test]
    fn manifest_records_the_template_version_and_owned_files() {
        let g = generate_with(None);
        let v: serde_json::Value =
            serde_json::from_str(&find(&g, ".espforge/manifest.json").content).unwrap();
        assert_eq!(
            v["template_version"],
            espforge_espgenerate_template::VERSION
        );
        assert!(v["files"].get("Cargo.toml").is_some());
        assert!(v["files"].get("README.txt").is_some());
        assert!(v["files"].get(".espforge/manifest.json").is_none());
        assert!(
            v["files"].get(".cargo/config.toml").is_none(),
            "seed-once files are not tracked"
        );
    }

    #[test]
    fn example_overlay_flows_through_generate() {
        let g = generate_with(Some("04.Communication/i2c_scanner"));
        assert!(find(&g, "diagram.json").content.contains("chip-chip"));
        assert!(g.assets.iter().any(|a| a.path == "chip.wasm"));
        assert!(find(&g, "README.txt").content.contains("chip.wasm"));
    }

    #[test]
    fn write_all_creates_files_assets_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        write_all(
            dir.path(),
            &generate_with(Some("04.Communication/i2c_scanner")),
        )
        .unwrap();
        assert!(dir.path().join("src/lib.rs").is_file());
        assert!(dir.path().join("README.txt").is_file());
        assert!(dir.path().join(".espforge/manifest.json").is_file());
        assert!(
            fs::read(dir.path().join("chip.wasm"))
                .unwrap()
                .starts_with(b"\0asm")
        );
    }

    #[test]
    fn rewriting_unchanged_output_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
    }

    #[test]
    fn edited_owned_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "// my edit").unwrap();
        let err = write(dir.path(), &generate_with(None).artifacts).unwrap_err();
        assert!(err.to_string().contains("drift"));
    }

    #[test]
    fn edited_seed_once_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        fs::write(dir.path().join("rust-toolchain.toml"), "# mine").unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("rust-toolchain.toml")).unwrap(),
            "# mine"
        );
    }

    #[test]
    fn assets_follow_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let seed = Asset::seed_once("chip.wasm", vec![0, 97, 115, 109]);
        write_assets(dir.path(), std::slice::from_ref(&seed)).unwrap();
        fs::write(dir.path().join("chip.wasm"), b"mine").unwrap();
        write_assets(dir.path(), std::slice::from_ref(&seed)).unwrap();
        assert_eq!(fs::read(dir.path().join("chip.wasm")).unwrap(), b"mine");
    }

    #[test]
    fn old_flat_manifests_still_drive_drift_detection() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".espforge")).unwrap();
        fs::write(
            dir.path().join(".espforge/manifest.json"),
            r#"{"src/lib.rs":"0000000000000000"}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "// edited").unwrap();
        assert!(write(dir.path(), &generate_with(None).artifacts).is_err());
    }
}

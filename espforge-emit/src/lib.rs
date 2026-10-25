//! Turns a resolved project into files.
//!
//! * [`codegen`]        - the typed backends: every emitted file
//! * [`project`]        - overlay layering and `README.txt`
//! * [`runtime_source`] - published vs local `espforge-runtime`
//! * [`manifest`]       - ownership manifest for drift detection
//!
//! IO boundary: [`generate_from_board`] / [`project::layer_overlay`] are pure
//! (in-memory `Artifact`/`Asset` in, data out; no `std::fs`, no
//! `std::process`). Only [`write`] / [`write_assets`] / [`write_all`] (and
//! `build.rs::execute` / `cargo_build` in the CLI) touch disk or processes.
//!
//! Host-only: never depend on `espforge-runtime` from this crate.

pub mod codegen;
pub mod manifest;
pub mod project;
pub mod runtime_source;

use anyhow::Result;
use espforge_model::{Artifact, Asset, Board, Meta, Ownership};
use espforge_options::ResolvedOptions;
use std::fs;
use std::path::Path;

pub use project::OverlayFile;
pub use runtime_source::RuntimeSource;

/// A machine-owned file was edited since espforge wrote it, so the write
/// refuses instead of clobbering the user's change.
///
/// Typed (rather than a bare `bail!` message) so the caller's exit-code
/// decision is a `downcast_ref`, not a substring match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestDrift {
    /// The artifact path, as it appears in the manifest.
    pub path: String,
}

impl std::fmt::Display for ManifestDrift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "refusing to overwrite edited file `{}` (drift detected). \
             Restore it or delete it to regenerate.",
            self.path
        )
    }
}

impl std::error::Error for ManifestDrift {}

/// Everything a build writes: text files and binary assets.
#[derive(Debug, Clone, Default)]
pub struct Generated {
    pub artifacts: Vec<Artifact>,
    pub assets: Vec<Asset>,
}

/// The project for `board`: every file from the typed backends
/// ([`codegen::generate`]), then the example overlay, then `README.txt` and
/// `.espforge/manifest.json` -- so the manifest describes what is actually
/// written.
///
/// Pure: no filesystem access.
pub fn generate_from_board(
    board: &Board,
    core: &Meta,
    resolved: &ResolvedOptions,
    runtime_source: &RuntimeSource,
    overlay: &[OverlayFile],
) -> Result<Generated> {
    let mut rendered = project::Rendered {
        artifacts: codegen::generate(board, core, resolved, runtime_source)?,
        assets: Vec::new(),
    };
    project::layer_overlay(board, resolved, &mut rendered, overlay)?;
    finish(rendered, core)
}

/// Add the README and the ownership manifest to a rendered project.
fn finish(rendered: project::Rendered, core: &Meta) -> Result<Generated> {
    let mut artifacts = rendered.artifacts;
    let assets = rendered.assets;
    artifacts.push(project::readme(&core.crate_name(), &artifacts, &assets));
    let manifest = manifest::emit_manifest(&artifacts, espforge_espgenerate_template::VERSION)?;
    artifacts.push(manifest);
    Ok(Generated { artifacts, assets })
}

/// Write artifacts into `out_dir`, respecting ownership:
///
/// Impure: creates directories and writes files under `out_dir`.
/// - `Owned` files are overwritten, unless the previous manifest shows the
///   file was edited since we wrote it (drift): then we refuse.
/// - `SeedOnce` files are written only if absent.
/// - `NotOwned` files are never written.
pub fn write(out_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let prev = manifest::read_prev_manifest(out_dir)?;
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
                            return Err(ManifestDrift {
                                path: a.path.clone(),
                            }
                            .into());
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
///
/// Impure: creates directories and writes files under `out_dir`.
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
///
/// Impure: writes under `out_dir`.
pub fn write_all(out_dir: &Path, generated: &Generated) -> Result<()> {
    write(out_dir, &generated.artifacts)?;
    write_assets(out_dir, &generated.assets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{Console, ResourceGraph, Runtime};
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

    /// A project with no components: every backend except the wiring is
    /// exercised, and the example overlay is the only hardware-shaped input.
    fn board(meta: &Meta) -> Board {
        Board {
            meta: meta.clone(),
            resources: ResourceGraph::default(),
            components: Vec::new(),
            services: Vec::new(),
        }
    }

    fn generate_with(example: Option<&str>) -> Generated {
        let meta = core();
        let opts =
            espforge_options::resolve(&meta, &Answers::empty(0..0), &DerivedInputs::default())
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
            .map(|(path, contents)| OverlayFile { path, contents })
            .collect();
        generate_from_board(
            &board(&meta),
            &meta,
            &opts,
            &RuntimeSource::Published,
            &overlay,
        )
        .unwrap()
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
            ".cargo/esp-config.toml",
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
        assert!(find(&g, "wokwi.toml").content.contains("[[chip]]"));
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

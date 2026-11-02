//! Turns a resolved project into files.
//!
//! * [`codegen`]        - the typed backends: every emitted file
//! * [`project`]        - overlay layering and `README.txt`
//! * [`runtime_source`] - published vs local `espforge-runtime`
//! * [`manifest`]       - ownership manifest for drift detection
//!
//! IO boundary: [`generate_from_board`] is pure (in-memory `Artifact`/`Asset`
//! in, data out; no `std::fs`, no `std::process`). Only [`write`] /
//! [`write_assets`] / [`write_all`] touch disk.
//!
//! Host-only: never depend on `espforge-runtime` from this crate.

pub mod artifact;
pub mod codegen;
pub mod manifest;
pub mod project;
pub mod runtime_source;

use anyhow::Result;
use espforge_elements::{Board, Meta, ResolvedOptions};
use std::fs;
use std::path::Path;

pub use artifact::{Artifact, Asset, Ownership};
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
    let manifest = manifest::emit_manifest(&artifacts, env!("CARGO_PKG_VERSION"))?;
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
    use espforge_elements::{Answers, Console, DerivedInputs, Runtime, resolve};

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
            resources: espforge_elements::ResourceGraph::default(),
            components: Vec::new(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn generate_with(example: Option<&str>) -> Generated {
        let meta = core();
        let opts = resolve(&meta, &Answers::empty(0..0), &DerivedInputs::default()).unwrap();
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
    }

    #[test]
    fn manifest_records_the_template_version_and_owned_files() {
        let g = generate_with(None);
        let v: serde_json::Value =
            serde_json::from_str(&find(&g, ".espforge/manifest.json").content).unwrap();
        assert_eq!(v["template_version"], env!("CARGO_PKG_VERSION"));
        assert!(v["files"].get("Cargo.toml").is_some());
        assert!(v["files"].get("README.txt").is_some());
        assert!(v["files"].get(".espforge/manifest.json").is_none());
        assert!(
            v["files"].get(".cargo/config.toml").is_none(),
            "seed-once files are not tracked"
        );
    }

    #[test]
    fn write_all_creates_files_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        write_all(dir.path(), &generate_with(None)).unwrap();
        assert!(dir.path().join("src/lib.rs").is_file());
        assert!(dir.path().join("README.txt").is_file());
        assert!(dir.path().join(".espforge/manifest.json").is_file());
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
}

//! Turns a resolved project into files.
//!
//! * [`codegen`]        - the typed backends: every emitted file
//! * [`project`]        - overlay layering and `README.txt`
//! * [`runtime_source`] - published vs local `espforge-runtime`
//!
//! IO boundary: [`generate_from_board`] is pure (in-memory `Artifact`/`Asset`
//! in, data out; no `std::fs`, no `std::process`). Only [`write`] /
//! [`write_assets`] / [`write_all`] touch disk.
//!
//! The output directory is disposable: everything in it is rewritten on every
//! build. The files a user controls live next to the `.espf` and reach the
//! generator as [`OverlayFile`]s.
//!
//! Host-only: never depend on `espforge-runtime` from this crate.

pub mod artifact;
pub mod codegen;
pub mod project;
pub mod runtime_source;

use anyhow::Result;
use espforge_elements::{Board, Meta, ResolvedOptions};
use std::fs;
use std::path::Path;

pub use artifact::{Artifact, Asset};
pub use project::OverlayFile;
pub use runtime_source::RuntimeSource;

/// Everything a build writes: text files and binary assets, plus what the
/// caller may want to tell the user about.
#[derive(Debug, Clone, Default)]
pub struct Generated {
    pub artifacts: Vec<Artifact>,
    pub assets: Vec<Asset>,
    /// One line per overlay decision worth reporting (a managed `Cargo.toml`
    /// key the user's `Cargo.extra.toml` tried to change).
    pub warnings: Vec<String>,
}

/// The project for `board`: every file from the typed backends
/// ([`codegen::generate`]), the project's overlay files, then `README.txt`.
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
        warnings: Vec::new(),
    };
    project::layer_overlay(board, resolved, &mut rendered, overlay)?;
    finish(rendered, core)
}

/// Add the README to a rendered project.
fn finish(rendered: project::Rendered, core: &Meta) -> Result<Generated> {
    let mut artifacts = rendered.artifacts;
    let assets = rendered.assets;
    artifacts.push(project::readme(&core.crate_name()));
    Ok(Generated {
        artifacts,
        assets,
        warnings: rendered.warnings,
    })
}

/// Write artifacts into `out_dir`, overwriting whatever is there.
///
/// Impure: creates directories and writes files under `out_dir`.
pub fn write(out_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    for a in artifacts {
        let path = out_dir.join(&a.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &a.content)?;
    }
    Ok(())
}

/// Write binary assets into `out_dir`, overwriting whatever is there.
///
/// Impure: creates directories and writes files under `out_dir`.
pub fn write_assets(out_dir: &Path, assets: &[Asset]) -> Result<()> {
    for a in assets {
        let path = out_dir.join(&a.path);
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

    /// A project with no devices: every backend except the wiring is
    /// exercised, and the example overlay is the only hardware-shaped input.
    fn board(meta: &Meta) -> Board {
        Board {
            meta: meta.clone(),
            resources: espforge_elements::ResourceGraph::default(),
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
    fn write_all_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        write_all(dir.path(), &generate_with(None)).unwrap();
        assert!(dir.path().join("src/lib.rs").is_file());
        assert!(dir.path().join("README.txt").is_file());
    }

    #[test]
    fn rewriting_unchanged_output_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
    }

    /// `build/` is disposable: an edited generated file is simply rewritten.
    #[test]
    fn an_edited_generated_file_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "// my edit").unwrap();
        write(dir.path(), &generate_with(None).artifacts).unwrap();
        assert!(
            fs::read_to_string(dir.path().join("src/lib.rs"))
                .unwrap()
                .contains("pub mod")
                || fs::read_to_string(dir.path().join("src/lib.rs"))
                    .unwrap()
                    .contains("espforge")
        );
    }
}

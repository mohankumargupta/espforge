//! The typed codegen that replaced the project-template stack (Phase 3).
//!
//! [`plan::FilePlan`] is the single list of emitted files and their owners;
//! [`generate`] walks it and asks one backend per [`plan::Source`] for the
//! content. [`equiv`] is the semantic comparator the dual-run gate uses while
//! the old and new producers coexist, and [`comments`] puts back the comments
//! `syn` cannot carry in the Rust files.
//!
//! Landed: the four Rust files ([`rust`], increment 2), the data files
//! ([`cargo`], [`config`], [`wokwi`], [`diagram`], increment 3) and the
//! example-overlay token resolver ([`overlay`], increment 4). The template
//! tree, the template crate's project files and the old `Hardware` projection
//! are deleted.

use anyhow::{Context, Result};
use espforge_model::{Artifact, Board, Meta};
use espforge_options::ResolvedOptions;
use std::collections::BTreeMap;

use crate::runtime_source::RuntimeSource;
use plan::{FilePlan, Source};

pub mod cargo;
pub mod chip;
pub mod comments;
pub mod config;
pub mod diagram;
pub mod equiv;
pub mod overlay;
pub mod plan;
pub mod rust;
#[cfg(test)]
pub mod test_support;
pub mod wokwi;

/// Every file of the project, in [`FilePlan`] order, with the plan's ownership.
///
/// The Rust backend resolves its own chip for `build.rs`; the data backends
/// share one [`chip::Chip`], so `esp-metadata-generated` is consulted twice per
/// build rather than once per file.
pub fn generate(
    board: &Board,
    meta: &Meta,
    opts: &ResolvedOptions,
    runtime: &RuntimeSource,
) -> Result<Vec<Artifact>> {
    let chip = chip::Chip::resolve(&meta.chip)?;
    let rust: BTreeMap<String, String> = rust::generate(board, meta, opts)?
        .into_iter()
        .map(|artifact| (artifact.path, artifact.content))
        .collect();

    let plan = FilePlan::for_project(opts);
    let mut artifacts = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let content = match file.source {
            Source::Rust => rust
                .get(file.out_path)
                .with_context(|| format!("the Rust backend has no `{}`", file.out_path))?
                .clone(),
            Source::Cargo => cargo::manifest(meta, opts, runtime, &chip)?,
            Source::CargoConfig => config::cargo_config(opts, &chip)?,
            Source::EspConfig => config::esp_config().to_string(),
            Source::Toolchain => config::toolchain(&chip)?,
            Source::Wokwi => wokwi::build(meta, &chip)?,
            Source::Diagram => diagram::build(&chip)?,
        };
        artifacts.push(Artifact {
            path: file.out_path.to_string(),
            content,
            ownership: file.ownership,
        });
    }
    Ok(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{Console, Ownership, ResourceGraph, Runtime};

    use crate::runtime_source::RuntimeSource;
    use espforge_options::{Answers, DerivedInputs};

    /// A project with no hardware: enough for the dispatcher and the data
    /// backends, which do not read components.
    fn hello() -> (Board, Meta, ResolvedOptions) {
        let meta = Meta {
            name: "hello world".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        };
        let opts =
            espforge_options::resolve(&meta, &Answers::empty(0..0), &DerivedInputs::default())
                .unwrap();
        let board = Board {
            meta: meta.clone(),
            resources: ResourceGraph::default(),
            components: Vec::new(),
            services: Vec::new(),
        };
        (board, meta, opts)
    }

    #[test]
    fn every_planned_file_is_produced_with_the_planned_owner() {
        let (board, meta, opts) = hello();
        let generated = generate(&board, &meta, &opts, &RuntimeSource::Published).unwrap();
        let paths: Vec<&str> = generated.iter().map(|a| a.path.as_str()).collect();
        let plan = FilePlan::for_project(&opts);
        let planned: Vec<&str> = plan.paths().collect();
        assert_eq!(paths, planned, "the plan and the output agree");

        for artifact in &generated {
            let file = plan.get(&artifact.path).unwrap();
            assert_eq!(artifact.ownership, file.ownership, "{}", artifact.path);
        }
        assert_eq!(plan.get("Cargo.toml").unwrap().ownership, Ownership::Owned);
        assert_eq!(
            plan.get(".cargo/config.toml").unwrap().ownership,
            Ownership::SeedOnce
        );
    }

    #[test]
    fn the_generated_files_parse() {
        let (board, meta, opts) = hello();
        let generated = generate(&board, &meta, &opts, &RuntimeSource::Published).unwrap();
        for artifact in &generated {
            match artifact.path.as_str() {
                "diagram.json" => {
                    serde_json::from_str::<serde_json::Value>(&artifact.content).unwrap();
                }
                path if path.ends_with(".rs") => {
                    syn::parse_file(&artifact.content).unwrap();
                }
                path if path.ends_with(".toml") => {
                    artifact
                        .content
                        .parse::<toml_edit::DocumentMut>()
                        .unwrap_or_else(|e| panic!("{path}: {e}"));
                }
                _ => {}
            }
        }
    }

    #[test]
    fn a_missing_rust_file_from_the_backend_is_an_error_not_a_gap() {
        // The plan asks for all four Rust files; `rust::generate` must supply
        // them. This pins the contract the dispatcher relies on.
        let (board, meta, opts) = hello();
        let rust = rust::generate(&board, &meta, &opts).unwrap();
        let paths: Vec<&str> = rust.iter().map(|a| a.path.as_str()).collect();
        let plan = FilePlan::for_project(&opts);
        for file in plan.files.iter().filter(|f| f.source == Source::Rust) {
            assert!(paths.contains(&file.out_path), "missing {}", file.out_path);
        }
    }
}

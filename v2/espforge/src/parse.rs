//! The `parse` stage of the pipeline (ADR-005): YAML → typed `Project`.
//!
//! Not yet span-aware at the per-field level — serde gives us whole-document
//! errors. Per-node `Span`s are populated where cheap (the owning crate can
//! thread byte offsets through serde's `Spanned` later); for now instance spans
//! default. Span-aware diagnostics for *validation* (ref resolution, double
//! claims) are added in the `validate` stage (Phase 3, ADR-009).

use espforge_model::project::Project;
use std::path::Path;

/// Parse a project YAML file into a typed `Project`.
///
/// Errors here are I/O or document-shape failures (anyhow). Semantic validation
/// (unknown drivers, unresolved refs, double claims) is a separate stage.
pub fn parse_file(path: &Path) -> anyhow::Result<Project> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    parse_str(&text)
}

pub fn parse_str(text: &str) -> anyhow::Result<Project> {
    let project: Project = serde_yaml_ng::from_str(text)
        .map_err(|e| anyhow::anyhow!("failed to parse project YAML: {e}"))?;
    Ok(project)
}

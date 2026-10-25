//! Turning a validated spec into files on disk: read the user's directory,
//! generate, write with ownership, and write the app logic.
//!
//! This is where a failure is classified. The binary must know whether a
//! problem is the user's (exit 1, already explained) or ours (exit 2, please
//! report it); deciding that here, from typed errors, keeps the binary from
//! matching on error text.

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use espforge_diagnostics::Diagnostic;
use espforge_emit::ManifestDrift;
use espforge_emit::OverlayFile;
use espforge_emit::codegen::overlay::OverlayError;

use crate::pipeline::Checked;
use crate::source::Source;

/// Why writing a project stopped.
#[derive(Debug)]
pub enum ProjectError {
    /// Located problems building the effective spec; the caller renders them.
    Spec(Vec<Diagnostic>),
    /// The user's spec or directory is at fault; the message is the report.
    User(String),
    /// Something broke inside espforge or the OS.
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for ProjectError {
    fn from(e: anyhow::Error) -> Self {
        ProjectError::Internal(e)
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Internal(e.into())
    }
}

/// What a successful write produced, for the caller's one-line summary.
#[derive(Debug)]
pub struct Written {
    pub crate_name: String,
}

/// Generate `checked`'s project under `out` and copy the user's app logic.
///
/// Impure: reads the project directory, writes under `out`.
pub fn generate(
    source: &Source,
    checked: &Checked,
    board: &espforge_model::Board,
    runtime_path: Option<PathBuf>,
    out: &Path,
) -> Result<Written, ProjectError> {
    let runtime = crate::source::runtime_source(runtime_path).map_err(ProjectError::User)?;
    let overlay_owned =
        crate::source::overlay_files(source).map_err(|e| ProjectError::User(format!("{e}")))?;
    let overlay: Vec<OverlayFile> = overlay_owned
        .iter()
        .map(|(path, contents)| OverlayFile { path, contents })
        .collect();
    let app = crate::source::app_logic(source).map_err(ProjectError::User)?;

    let generated = espforge_emit::generate_from_board(
        board,
        &checked.meta,
        &checked.resolved,
        &runtime,
        &overlay,
    )
    .map_err(|e| match e.downcast_ref::<OverlayError>() {
        // A placeholder espforge does not support is the example's (the
        // user's) input, not an internal failure.
        Some(overlay) => ProjectError::User(overlay.to_string()),
        None => ProjectError::Internal(e),
    })?;

    espforge_emit::write_all(out, &generated).map_err(|e| {
        match e.downcast_ref::<ManifestDrift>() {
            // The user edited a machine-owned file.
            Some(drift) => ProjectError::User(drift.to_string()),
            None => ProjectError::Internal(e),
        }
    })?;

    // The user's app logic is copied verbatim on every build.
    std::fs::create_dir_all(out.join("src"))?;
    std::fs::write(out.join("src").join("app.rs"), app)?;

    Ok(Written {
        crate_name: checked.meta.crate_name(),
    })
}

/// Write the effective spec (file + overrides) to `path`.
///
/// Impure: writes `path`. A document that cannot be built returns the located
/// problems for the caller to render; those should be impossible after
/// `validate`, which is exactly why they stay diagnostics.
pub fn write_spec(checked: &Checked, path: &Path) -> Result<(), ProjectError> {
    let doc = crate::spec::effective_document(
        &checked.fe.parsed.sections,
        &checked.meta,
        &checked.answers,
    )
    .map_err(|errors| {
        ProjectError::Spec(errors.iter().map(|e| e.to_diagnostic()).collect::<Vec<_>>())
    })?;
    let text = crate::spec::render(&doc).map_err(|e| ProjectError::Internal(anyhow!("{e}")))?;
    std::fs::write(path, text)
        .map_err(|e| ProjectError::Internal(anyhow!("cannot write '{}': {e}", path.display())))?;
    Ok(())
}

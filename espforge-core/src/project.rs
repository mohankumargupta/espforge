//! Turning a validated spec into files on disk: read the user's directory,
//! generate, write with ownership, and write the app logic.
//!
//! This is where a failure is classified. The binary must know whether a
//! problem is the user's (exit 1, already explained) or ours (exit 2, please
//! report it); deciding that here, from typed errors, keeps the binary from
//! matching on error text.

use std::path::{Path, PathBuf};

use espforge_diagnostics::Diagnostic;
use espforge_emit::ManifestDrift;
use espforge_emit::OverlayFile;
use espforge_emit::codegen::overlay::OverlayError;

use crate::answers::Overrides;
use crate::pipeline::{self, Checked, Invalid};
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

impl From<Invalid> for ProjectError {
    fn from(e: Invalid) -> Self {
        match e {
            Invalid::Diagnostics(diagnostics) => ProjectError::Spec(diagnostics),
            Invalid::Message(message) => ProjectError::User(message),
        }
    }
}

/// What a successful write produced, for the caller's summary.
#[derive(Debug)]
pub struct Written {
    pub crate_name: String,
    /// `note: ...` lines for flags that changed something.
    pub notes: Vec<String>,
}

/// Generate `source`'s project under `out`, with no flag overrides and the
/// published runtime.
///
/// Impure: reads the project directory, writes under `out`.
pub fn generate(source: &Source, out: &Path) -> Result<Written, ProjectError> {
    generate_with(source, out, &Overrides::default(), None)
}

/// [`generate`], with command-line overrides and an optional local runtime.
pub fn generate_with(
    source: &Source,
    out: &Path,
    overrides: &Overrides,
    runtime_path: Option<PathBuf>,
) -> Result<Written, ProjectError> {
    let checked = pipeline::validate(&source.text, overrides)?;
    write_checked(source, &checked, out, runtime_path)
}

/// Everything generation reads and validates, without writing anything.
///
/// This is the spec pipeline plus the app logic and overlay files. It stops
/// before [`espforge_emit::generate_from_board`], so a passing check does not
/// promise that generation or writing succeeds (an unsupported overlay
/// placeholder, an edited generated file): callers must not promise a
/// buildable project from it alone.
///
/// Pure apart from reading the user's directory.
pub fn check(source: &Source, overrides: &Overrides) -> Result<Vec<String>, ProjectError> {
    let checked = pipeline::validate(&source.text, overrides)?;
    crate::source::app_logic(source).map_err(ProjectError::User)?;
    crate::source::overlay_files(source, checked.resolved.is_on("keep-zig-source"))
        .map_err(ProjectError::User)?;
    Ok(checked.notes)
}

/// Emit and write an already validated spec.
pub fn write_checked(
    source: &Source,
    checked: &Checked,
    out: &Path,
    runtime_path: Option<PathBuf>,
) -> Result<Written, ProjectError> {
    let runtime = crate::source::runtime_source(runtime_path).map_err(ProjectError::User)?;
    let overlay_owned =
        crate::source::overlay_files(source, checked.resolved.is_on("keep-zig-source"))
            .map_err(|e| ProjectError::User(format!("{e}")))?;
    let overlay: Vec<OverlayFile> = overlay_owned
        .iter()
        .map(|(path, contents)| OverlayFile { path, contents })
        .collect();
    let app = crate::source::app_logic(source).map_err(ProjectError::User)?;

    let generated = espforge_emit::generate_from_board(
        &checked.board,
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

    // Keep the ESPF source that produced this build next to the manifest, so the
    // generated project always carries the exact document it came from.
    std::fs::write(out.join(".espforge").join("source.espf.txt"), &source.text)?;

    Ok(Written {
        crate_name: checked.meta.crate_name(),
        notes: checked.notes.clone(),
    })
}

/// Write a generated project for a source, without any flag overrides.
///
/// Impure: writes `out`. Kept for callers that only have a [`Source`].
pub fn generate_source(source: &Source, out: &Path) -> Result<Written, ProjectError> {
    generate(source, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::resolve_source;

    #[test]
    fn generate_writes_every_helloworld_file() {
        let source = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let written = generate(&source, &out).unwrap();
        assert_eq!(written.crate_name, "hello_world");
        for f in [
            "Cargo.toml",
            "build.rs",
            "src/lib.rs",
            "src/bin/main.rs",
            "src/app.rs",
            "README.txt",
            ".espforge/manifest.json",
            ".espforge/source.espf.txt",
        ] {
            assert!(out.join(f).is_file(), "missing {f}");
        }
        assert_eq!(
            std::fs::read_to_string(out.join(".espforge/source.espf.txt")).unwrap(),
            source.text
        );
        assert!(
            std::fs::read_to_string(out.join("src/app.rs"))
                .unwrap()
                .contains("Hello World!")
        );
        assert!(
            std::fs::read_to_string(out.join("Cargo.toml"))
                .unwrap()
                .contains("name = \"hello_world\"")
        );
    }

    #[test]
    fn rebuilding_is_fine_but_edited_owned_files_are_refused() {
        let source = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        generate(&source, &out).unwrap();
        generate(&source, &out).unwrap();
        std::fs::write(out.join("src/lib.rs"), "// mine").unwrap();
        assert!(matches!(
            generate(&source, &out),
            Err(ProjectError::User(_))
        ));
    }

    #[test]
    fn flags_override_the_file_in_the_output() {
        let source = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let overrides = Overrides {
            chip: Some("esp32s3".into()),
            runtime: Some(espforge_elements::Runtime::Embassy),
            console: Some(espforge_elements::Console::JtagSerial),
            ..Default::default()
        };
        let written = generate_with(&source, &out, &overrides, None).unwrap();
        assert_eq!(written.notes.len(), 3);
        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("\"esp32s3\"")
                && cargo.contains("\"jtag-serial\"")
                && cargo.contains("embassy"),
            "{cargo}"
        );
    }

    #[test]
    fn bad_flags_are_user_errors() {
        let source = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let overrides = Overrides {
            options: vec!["nope=1".into()],
            ..Default::default()
        };
        assert!(matches!(
            generate_with(&source, &out, &overrides, None),
            Err(ProjectError::User(_))
        ));
    }

    #[test]
    fn spec_errors_are_located() {
        let source = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        let bad = Source {
            name: source.name.clone(),
            text: source.text.replace("esp32c3", "esp99"),
            dir: None,
            example: source.example,
        };
        let err = generate(&bad, &tempfile::tempdir().unwrap().path().join("out")).unwrap_err();
        match err {
            ProjectError::Spec(diagnostics) => {
                assert_eq!(diagnostics[0].message, "unknown chip");
            }
            other => panic!("expected located spec errors, got {other:?}"),
        }
    }

    #[test]
    fn check_accepts_a_good_project_and_rejects_a_missing_app() {
        let source = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        let notes = check(&source, &Overrides::default()).unwrap();
        assert!(notes.is_empty());

        // The same spec on disk, with no app logic beside it.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.espf");
        std::fs::write(&file, &source.text).unwrap();
        let source = crate::source::from_file(&file).unwrap();
        assert!(matches!(
            check(&source, &Overrides::default()),
            Err(ProjectError::User(message)) if message.contains("app logic")
        ));
    }
}

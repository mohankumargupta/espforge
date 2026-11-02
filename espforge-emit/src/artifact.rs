//! In-memory generated files and the ownership rule that governs them.
//!
//! Emitters return [`Artifact`]s; [`crate::write`] decides what reaches disk.
//! Nothing here touches the filesystem.

/// An in-memory generated text file. Emitters return these; the writer owns IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Path relative to the output dir, e.g. `src/generated.rs`.
    pub path: String,
    pub content: String,
    pub ownership: Ownership,
}

/// An in-memory binary file (e.g. a prebuilt Wokwi `chip.wasm`), copied
/// verbatim. Assets are never tracked in the drift manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Path relative to the output dir.
    pub path: String,
    pub bytes: Vec<u8>,
    pub ownership: Ownership,
}

impl Asset {
    pub fn owned(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            ownership: Ownership::Owned,
        }
    }

    pub fn seed_once(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            ownership: Ownership::SeedOnce,
        }
    }
}

/// Who owns a generated path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// Regenerated every build and tracked in the manifest (drift detection).
    Owned,
    /// Written only if absent; never tracked, never clobbered.
    SeedOnce,
    /// Not written by espforge at all.
    NotOwned,
}

impl Artifact {
    pub fn owned(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
            ownership: Ownership::Owned,
        }
    }
    pub fn seed_once(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
            ownership: Ownership::SeedOnce,
        }
    }
}

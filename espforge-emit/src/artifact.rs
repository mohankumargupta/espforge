//! In-memory generated files.
//!
//! Emitters return [`Artifact`]s; [`crate::write`] decides what reaches disk.
//! Nothing here touches the filesystem.

/// An in-memory generated text file. Emitters return these; the writer owns IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Path relative to the output dir, e.g. `src/generated.rs`.
    pub path: String,
    pub content: String,
}

/// An in-memory binary file (e.g. a prebuilt Wokwi `chip.wasm`), copied
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Path relative to the output dir.
    pub path: String,
    pub bytes: Vec<u8>,
}

impl Artifact {
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

impl Asset {
    pub fn new(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
        }
    }
}

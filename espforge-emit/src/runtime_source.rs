//! Where the generated project's `espforge-runtime` dependency comes from.
//!
//! `--runtime-path` (or `ESPFORGE_RUNTIME_PATH`) points the generated
//! `Cargo.toml` at a local checkout; otherwise the published crate is used.
//! The version written is [`RUNTIME_VERSION`], the version this emitter is
//! built against.

/// Version of `espforge-runtime` written into generated manifests.
pub const RUNTIME_VERSION: &str = "0.6.0";

#[derive(Debug, Clone)]
pub enum RuntimeSource {
    /// Use the published crate from crates.io.
    Published,
    /// Path to the `espforge-runtime` crate directory, as it should appear
    /// in the generated Cargo.toml.
    Path(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_version_is_the_runtime_crate_version() {
        assert_eq!(RUNTIME_VERSION, "0.6.0");
    }
}

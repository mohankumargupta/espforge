//! espforge: CLI binary + parse/resolve/emit orchestration (host/std, ADR-007).
//!
//! This crate drives the pipeline (ADR-005): `parse` → `validate` → `resolve`
//! → `emit*`. It references `espforge-runtime` only by name inside emitted token
//! streams — never as a Cargo dependency (ADR-007).

pub mod parse;
pub mod pipeline;

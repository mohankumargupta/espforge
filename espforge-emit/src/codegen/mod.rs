//! The typed codegen that replaces the minijinja project template (Phase 3).
//!
//! Increment 1 adds the two seams this phase needs: a Rust [`plan::FilePlan`]
//! describing which files are emitted and who owns them, and
//! [`equiv`], the semantic comparator the dual-run gate uses while the old and
//! new producers coexist. Production still renders the template until the
//! backends land; nothing here changes emitted bytes.

pub mod equiv;
pub mod plan;

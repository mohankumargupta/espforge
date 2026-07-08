//! espforge-bindings: in-tree driver catalog + `generate` impls (ADR-006).
//!
//! For now this crate owns the **driver catalog** — the data-only metadata the
//! `validate`/`resolve` stages use (ADR-009). The `Driver` trait code-generation
//! impls land in a later phase.

pub mod catalog;

pub use catalog::catalog;

//! `espforge-model`: the domain model types shared by every crate in espforge v2.
//!
//! This crate is the leaf of the dependency graph (ADR-007): it depends on
//! neither host-side codegen crates nor the `no_std` runtime. Both the host
//! (`espforge`, `espforge-bindings`) and, indirectly, the emitted target code
//! depend on the contract defined here.
//!
//! Core responsibilities:
//! - **`ir`** — the `DeviceTree` intermediate representation that all emitters
//!   read (ADR-005). It is produced by the `validate` + `resolve` stages and is
//!   the single inspectable artifact of the pipeline.
//! - **`driver`** — the `Driver` trait + registry types describing how a
//!   capability is declared and code-generated (ADR-006).
//! - **`value`** — value objects: `PinRef`, `ResourceRef`, `Diag`, `Artifact`.
//! - **`project`** — the typed user-facing `Project` parsed from YAML (ADR-004).

pub mod driver;
pub mod ir;
pub mod project;
pub mod value;

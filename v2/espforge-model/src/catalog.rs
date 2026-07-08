//! Driver catalog: the metadata needed to *validate* a project (ADR-009), kept
//! separate from code generation (ADR-006).
//!
//! The `Driver` trait (in `driver.rs`) is about emitting code. Validation needs
//! a lighter, data-only description of each driver: its tier, the dependencies it
//! expects, and the pins/peripherals it claims. A `DriverSpec` is that
//! description. `espforge-bindings` owns the in-tree catalog; the `validate` /
//! `resolve` stages consume it to check `using:` against known kinds and to build
//! the dependency graph.

use crate::ir::{Access, DepKind, Tier};
use serde::{Deserialize, Serialize};

/// A data-only description of a driver, sufficient for validation + resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverSpec {
    /// Kind name selected by `using:` / `driver:`.
    pub kind: String,
    /// Component (reusable) or Device (terminal).
    pub tier: Tier,
    /// Dependencies this driver expects, each described by how to find it.
    #[serde(default)]
    pub deps: Vec<DepSpec>,
    /// Pins this driver claims by value (move-by-value, ADR-008). The YAML key
    /// under `with:` that supplies the pin ref, e.g. `pin`.
    #[serde(default)]
    pub pins: Vec<String>,
    /// Peripherals (buses) this driver claims by value. The YAML key under
    /// `with:` that supplies the bus ref, e.g. `bus`.
    #[serde(default)]
    pub peripherals: Vec<String>,
    /// Cross-cutting flags forced on when this driver is present.
    #[serde(default)]
    pub flags: SpecFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepSpec {
    /// The `with:` key that holds the reference, e.g. `bus`.
    pub key: String,
    /// What the reference must point at.
    pub kind: DepKind,
    /// How it is wired (Shared = by ref, Owned = by value).
    pub access: Access,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecFlags {
    #[serde(default)]
    pub has_alloc: bool,
    #[serde(default)]
    pub has_wifi: bool,
    #[serde(default)]
    pub needs_delay: bool,
    #[serde(default)]
    pub needs_stack: bool,
}

impl DriverSpec {
    /// Look up an expected dependency spec by its `with:` key.
    pub fn dep(&self, key: &str) -> Option<&DepSpec> {
        self.deps.iter().find(|d| d.key == key)
    }
}

//! The `Driver` trait and registry types (ADR-006).
//!
//! A driver is one capability — the single declaration that collapses what v1
//! spread across five files into one. It knows: its `kind` name (what `using:`
//! selects), which tier it is (Component vs Device), required Cargo features,
//! its dependency graph, and how to emit code from a `ResolvedInstance`.
//!
//! Discovery is an **explicit registry list** (`&[&dyn Driver]`), not `inventory`
//! + a `black_box` init hack (ADR-006). `espforge-bindings` holds the in-tree
//! list; the CLI indexes it by `kind`.

use crate::ir::ResolvedInstance;
use crate::value::{Artifact, Diag};
use std::fmt::Debug;

/// A driver declaration. Implemented once per capability (e.g. `led`,
/// `ssd1306`, `http`). Pure and host-side: it emits `Artifact`s from a resolved
/// instance; it never touches hardware or `espforge-runtime` directly (the
/// runtime is referenced only by name in the emitted token stream, ADR-007).
pub trait Driver: Debug + Send + Sync {
    /// The kind name selected by `using:` / `driver:` in YAML, e.g. `"led"`.
    fn kind(&self) -> &str;

    /// Which tier this driver belongs to (ADR-003). A `Device` is terminal.
    fn tier(&self) -> crate::ir::Tier;

    /// Cargo features this driver requires (e.g. `["embassy"]`, `["alloc"]`).
    fn required_features(&self) -> &[&str] {
        &[]
    }

    /// Cross-cutting flags this driver forces on when present (ADR-005).
    fn flags(&self) -> DriverFlags {
        DriverFlags::default()
    }

    /// Emit the code artifacts for one resolved instance of this driver.
    ///
    /// `ctx` carries shared generation context (target chip, runtime, the IR).
    /// Returns the artifacts that make up this instance's contribution (e.g. a
    /// generated `impl` block appended to `generated.rs`).
    fn generate(&self, inst: &ResolvedInstance, ctx: &GenContext) -> Result<Vec<Artifact>, Diag>;
}

/// Flags a driver can assert about the whole project when it is present.
#[derive(Debug, Clone, Copy, Default)]
pub struct DriverFlags {
    pub has_alloc: bool,
    pub has_wifi: bool,
    pub needs_delay: bool,
    pub needs_stack: bool,
}

/// Shared context passed to every driver's `generate`.
#[derive(Debug, Clone)]
pub struct GenContext {
    /// Target chip, e.g. `esp32`.
    pub target: Option<String>,
    /// Embassy vs blocking.
    pub is_embassy: bool,
}

/// An explicit, in-tree registry of drivers (ADR-006). The CLI holds one of
/// these and indexes it by `kind`. No link-time discovery magic.
#[derive(Debug)]
pub struct Registry {
    drivers: Vec<&'static dyn Driver>,
}

impl Registry {
    pub fn new(drivers: &'static [&'static dyn Driver]) -> Self {
        Self { drivers: drivers.to_vec() }
    }

    /// Look up a driver by its `kind` name.
    pub fn get(&self, kind: &str) -> Option<&'static dyn Driver> {
        self.drivers.iter().copied().find(|d| d.kind() == kind)
    }

    pub fn all(&self) -> &[&'static dyn Driver] {
        &self.drivers
    }
}

/// A `Driver` that is implemented entirely inline (for tests / trivial drivers),
/// wrapping closures. Kept in this crate so `espforge-model` is self-contained
/// for unit tests (ADR-010) without pulling in `espforge-bindings`.
#[derive(Debug)]
pub struct FnDriver {
    pub kind: &'static str,
    pub tier: crate::ir::Tier,
    pub features: Vec<&'static str>,
    pub flags: DriverFlags,
    #[allow(clippy::type_complexity)]
    pub r#gen: fn(&ResolvedInstance, &GenContext) -> Result<Vec<Artifact>, Diag>,
}

impl Driver for FnDriver {
    fn kind(&self) -> &str {
        self.kind
    }
    fn tier(&self) -> crate::ir::Tier {
        self.tier
    }
    fn required_features(&self) -> &[&str] {
        &self.features
    }
    fn flags(&self) -> DriverFlags {
        self.flags
    }
    fn generate(
        &self,
        inst: &ResolvedInstance,
        ctx: &GenContext,
    ) -> Result<Vec<Artifact>, Diag> {
        (self.r#gen)(inst, ctx)
    }
}

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

use crate::backend;
use crate::codegen;
use crate::ir::ResolvedInstance;
use crate::value::{Artifact, Diag};
use std::fmt::Debug;

/// Per-instance construction snippet a driver emits (ADR-005/008). The emitter
/// collects these and assembles the `Components { .. }` / `Devices { .. }`
/// struct literals in `main.rs` from them — this is what makes the emitter
/// driver-driven (ADR-006): adding a driver is a one-file change, no central
/// mapping to edit.
#[derive(Debug, Clone)]
pub struct Construction {
    /// The sanitized field name in `Components`/`Devices` (matches the
    /// `generated.rs` struct field).
    pub field: String,
    /// The full initializer expression, e.g.
    /// `espforge_runtime::components::Led::new(registry.peripherals.GPIO18, false)`.
    /// References `registry` (a `PeripheralRegistry`) and any shared components
    /// by their field name.
    pub expr: String,
}

impl Construction {
    /// Build a construction for `inst` from a backend-rendered expression.
    /// Sanitizes the instance id into the struct field name (ADR-008).
    pub fn for_instance(inst: &ResolvedInstance, expr: impl Into<String>) -> Self {
        Construction {
            field: codegen::sanitize(&inst.id),
            expr: expr.into(),
        }
    }
}

/// A driver declaration. Implemented once per capability (e.g. `led`,
/// `ssd1306`, `http`). Pure and host-side: it emits `Artifact`s from a resolved
/// instance; it never touches hardware or `espforge-runtime` directly (the
/// runtime is referenced only by name in the emitted token stream, ADR-007).
pub trait Driver: Debug + Send + Sync {
    /// The kind name selected by `using:` / `driver:` in YAML, e.g. `"led"`.
    fn kind(&self) -> &str;

    /// Which tier this driver belongs to (ADR-003). A `Device` is terminal.
    fn tier(&self) -> crate::ir::Tier;

    /// The concrete runtime struct name this driver constructs, e.g. `I2cBus`
    /// (not the YAML `kind`, which may differ). Used to type the generated
    /// `Components`/`Devices` struct fields (ADR-008). Defaults to `kind`.
    fn type_name(&self) -> &str {
        self.kind()
    }

    /// Instance-specific concrete type for the generated `Components` field
    /// (design §20: comms types are parametric over `Dm` and SPI may wrap a
    /// `SpiDevice` when a CS pin is declared). Defaults to `type_name()`;
    /// drivers whose constructed type varies per-instance override this.
    /// Must match the expression returned by `construct` exactly.
    fn type_name_for(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> String {
        self.type_name().to_string()
    }

    /// Cargo features this driver requires (e.g. `["embassy"]`, `["alloc"]`).
    /// These are *project-level* features of the generated firmware project,
    /// distinct from `runtime_features()` (which gates `espforge-runtime`
    /// modules). See design §19.2.
    fn required_features(&self) -> &[&str] {
        &[]
    }

    /// The `espforge-runtime` **module feature names** this driver needs
    /// (design §19.2). Each name gates a `pub mod` in `espforge-runtime`
    /// (`#[cfg(feature = "led")] pub mod led;`) and, for drivers that wrap an
    /// external crate, enables the matching optional dependency. Defaults to
    /// `[kind()]` so a plain driver is one line; override only for rare cases
    /// where multiple `using:` kinds share one feature set (§19.9). Returned
    /// owned to avoid lifetime gymnastics with `kind()`.
    fn runtime_features(&self) -> Vec<String> {
        vec![self.kind().to_string()]
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

    /// Emit the per-instance construction used to wire this instance into the
    /// `Components`/`Devices` struct literal in `main.rs` (ADR-008, move-by-
    /// value). Default panics so a driver must opt in; drivers that only emit
    /// artifacts (no runtime wiring) can leave the default.
    fn construct(&self, _inst: &ResolvedInstance, _ctx: &GenContext) -> Construction {
        Construction {
            field: String::new(),
            expr: String::new(),
        }
    }
}

/// Flags a driver can assert about the whole project when it is present.
#[derive(Debug, Clone, Copy, Default)]
pub struct DriverFlags {
    pub is_embassy: bool,
    pub has_alloc: bool,
    pub has_wifi: bool,
    pub needs_delay: bool,
    pub needs_stack: bool,
}

/// Shared context passed to every driver's `generate` / `construct`.
#[derive(Debug, Clone)]
pub struct GenContext {
    /// Target chip, e.g. `esp32`.
    pub target: Option<String>,
    /// Embassy vs blocking (IR-level flag; driver codegen uses `backend`).
    pub is_embassy: bool,
    /// Resolved peripherals, so drivers can resolve a claimed peripheral to its
    /// esp_hal field name (ADR-008 move-by-value wiring).
    pub peripherals: Vec<crate::ir::Peripheral>,
    /// The codegen backend (blocking now; embassy later). Drivers render
    /// backend-specific snippets through this rather than inlining `esp_hal`
    /// calls (ADR-008). Held as `&'static` so the context is cheap to clone.
    pub backend: &'static dyn backend::Backend,
}

/// An explicit, in-tree registry of drivers (ADR-006). The CLI holds one of
/// these and indexes it by `kind`. No link-time discovery magic.
#[derive(Debug)]
pub struct Registry {
    drivers: Vec<&'static dyn Driver>,
}

impl Registry {
    pub fn new(drivers: &[&'static dyn Driver]) -> Self {
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

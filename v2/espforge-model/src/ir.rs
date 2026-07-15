//! The `DeviceTree` intermediate representation (ADR-005).
//!
//! Produced by the `resolve` stage from a validated `Project` (ADR-004). It is
//! the single inspectable artifact that **all emitters read** — they never touch
//! the raw `Project` or YAML. By construction (because `validate` ran first) the
//! IR is valid: every reference resolves, no pin is double-claimed, and there are
//! no cycles.
//!
//! Moving ownership-by-value at runtime (ADR-008) is made possible *because* the
//! IR records exactly which peripheral each instance consumes, so the codegen
//! stage can generate per-project `new` signatures.

use crate::value::{PinRef, ResourceRef, Span};
use serde::{Deserialize, Serialize};

/// The validated, resolved project. Output of `resolve`; input to every emitter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTree {
    /// Project metadata (name, target chip, runtime).
    pub meta: Meta,
    /// All peripherals declared in `esp32:`, keyed by name.
    pub peripherals: Vec<Peripheral>,
    /// All component + device instances, in dependency order (see `init_order`).
    pub instances: Vec<ResolvedInstance>,
    /// Initialization order: indices into `instances`, topologically sorted so
    /// a dependency always precedes its consumer.
    pub init_order: Vec<usize>,
    /// Cargo features required by the resolved project (e.g. `embassy`,
    /// `alloc`, `wifi`).
    pub required_features: Vec<String>,
    /// Cross-cutting flags derived during resolve (ADR-005).
    pub flags: Flags,
    /// Top-level `esp32.wifi` config, if declared. Consumed by the emitter to
    /// build the singleton Stack (ADR-012); the Stack is implicit infrastructure,
    /// not a claimed peripheral.
    pub wifi: Option<crate::project::WifiConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub name: Option<String>,
    pub target: Option<String>,
    pub runtime: Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Blocking,
    Embassy,
}

/// A raw hardware peripheral (ADR-003). `claimed_by` records which instance
/// consumes it; the `validate` stage guarantees exactly one claim (ADR-008/009).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peripheral {
    pub name: String,
    pub kind: PeripheralKind,
    /// GPIO number for pins; peripheral index for buses.
    pub number: u32,
    /// The esp_hal peripheral field name, e.g. `GPIO18`, `I2C0`. Used by drivers
    /// to emit move-by-value wiring: `registry.peripherals.I2C0` (ADR-008).
    pub field: String,
    /// Bus configuration when `kind` is a bus.
    pub bus: Option<BusInit>,
    /// The instance that claimed this peripheral, if any.
    pub claimed_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeripheralKind {
    Pin,
    I2c,
    Spi,
    Uart,
    Wifi,
}

/// Bus wiring carried in the IR, precise per bus kind (model refactor C). Each
/// variant holds only the pins/params valid for that bus, so the emitters can
/// read typed fields instead of a flat `Option` bag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusInit {
    I2c(I2cInit),
    Spi(SpiInit),
    Uart(UartInit),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I2cInit {
    pub sda: Option<u32>,
    pub scl: Option<u32>,
    pub frequency_khz: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpiInit {
    pub mosi: Option<u32>,
    pub miso: Option<u32>,
    pub sclk: Option<u32>,
    pub cs: Option<u32>,
    pub mode: Option<u8>,
    pub frequency_khz: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UartInit {
    pub tx: Option<u32>,
    pub rx: Option<u32>,
    pub baud: Option<u32>,
}

/// A resolved component or device instance. Unlike `project::Instance`, the
/// `with` map has been validated and the `deps` carry *typed* access paths so
/// emitters know how to wire each dependency (e.g. "borrow `i2c_master` by
/// shared ref", or "take pin `GPIO18` by value").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedInstance {
    /// Instance name (the `$name`).
    pub id: String,
    /// Driver kind, e.g. `led`, `ssd1306`.
    pub kind: String,
    /// Whether this is a reusable component or a terminal device (ADR-003).
    pub tier: Tier,
    /// Driver-specific, validated parameters (opaque to the IR; the emitter
    /// re-parses against the driver schema). Kept as `Value` because the IR
    /// must not depend on per-driver types (ADR-006 single-file driver model).
    pub with: serde_yaml_ng::Value,
    /// Resolved dependencies, in wiring order.
    pub deps: Vec<Dependency>,
    /// Peripherals this instance claims by value (move-by-value, ADR-008).
    pub claims: Vec<ResourceRef>,
    /// Pins this instance claims by value.
    pub pins: Vec<PinRef>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// Reusable capability (hardware-backed or software-service). May be shared.
    Component,
    /// Terminal high-level driver. Consumed only by the app.
    Device,
}

/// A wiring edge from a consumer instance to something it depends on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// The name of the depended-upon instance or peripheral.
    pub name: String,
    /// What kind of thing is depended on.
    pub kind: DepKind,
    /// How it is wired: shared (by ref) vs owned (by value).
    pub access: Access,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepKind {
    Instance,
    Peripheral,
    Pin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Access {
    /// Shared: lent by reference (e.g. one `I2cDevice` bus to many displays).
    Shared,
    /// Owned: moved by value into the consumer (e.g. a control pin).
    Owned,
}

/// Cross-cutting flags computed during resolve and consumed by emitters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flags {
    /// Embassy runtime selected.
    pub is_embassy: bool,
    /// A heap allocator is required (e.g. for embassy net buffers).
    pub has_alloc: bool,
    /// WiFi capability present.
    pub has_wifi: bool,
    /// Some instance needs a `Delay` (e.g. blocking timing).
    pub needs_delay: bool,
    /// Some instance needs the embassy TCP/IP `Stack` (http/https/websockets).
    pub needs_stack: bool,
}

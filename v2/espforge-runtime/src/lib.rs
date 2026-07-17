//! espforge-runtime: `no_std` runtime implementations of each capability.
//!
//! Unlike esphome, this crate keeps **separate `components` and `devices`
//! modules** (ADR-007), mirroring the three-tier domain spine (ADR-003).
//!
//! This is a target-only crate: the types are concrete `esp-hal` types, so the
//! generated project wires instances move-by-value (ADR-008) with no generics.
//! `Logger` and `Delay` are espforge's own types (mirroring v1's
//! `espforge_platform`), not thin esp-hal re-exports.

#![no_std]

// `alloc` is available in generated projects that enable the `has_alloc` flag
// (e.g. the `http` software-service, ADR-012); this lets runtime modules use
// `String`/`Vec`. The global allocator is provided by the generated project
// (esp-alloc), not by this crate.
#[cfg(feature = "alloc")]
extern crate alloc;

// The `components` namespace is always present; individual capability modules
// are gated per-sub-module in `components/mod.rs` (design §19.1). Gating the
// whole namespace on `any(feature = ...)` here caused the module to be silently
// compiled out (and `espforge_runtime::components::X` to fail to resolve) when a
// new component feature was added but forgotten in the list — so we keep the
// namespace unconditionally compiled and let the per-struct `#[cfg]` gates do
// the real selection. The emitter only ever references a component whose
// feature is enabled, so there is no dead-code cost.
pub mod components;
// Software-service capabilities (Http, future mqtt/websockets) live in
// `services/` (ADR-013): hardware-backed components live in `components/`, and
// the two are kept apart internally. `services` is always compiled; individual
// modules are per-feature gated, and re-exported into `components` so the
// generated `ctx.components.http` accessor is uniform.
pub mod services;
// The `devices` namespace is always present, mirroring `components` above;
// individual device submodules are gated per-sub-module in `devices/mod.rs`
// (design §19.1). Gating the whole namespace here caused the module to be
// silently compiled out (and `espforge_runtime::devices::X` to fail to
// resolve) when a device feature was enabled but forgotten from the list — so
// we keep the namespace unconditionally compiled and let the per-struct
// `#[cfg]` gates do the real selection. The emitter only ever references a
// device whose feature is enabled, so there is no dead-code cost.
pub mod devices;

/// Re-exported under the `signal` feature so the generated project's `signal!`
/// macro (emitted by the generator) can name `embassy_sync::signal::Signal`
/// without the project adding `embassy-sync` itself.
#[cfg(feature = "signal")]
pub use embassy_sync;

/// Logging handle stored on the `Context` (ADR-008 stable API). Forwards to the
/// `log` facade; the generated project installs the sink (e.g. `esp-println`).
#[derive(Clone, Copy)]
pub struct Logger;

impl Logger {
    pub fn new() -> Self {
        Logger
    }
    pub fn info(&self, msg: impl core::fmt::Display) {
        log::info!("{}", msg);
    }
    pub fn warn(&self, msg: impl core::fmt::Display) {
        log::warn!("{}", msg);
    }
    pub fn error(&self, msg: impl core::fmt::Display) {
        log::error!("{}", msg);
    }
}

/// Delay handle stored on the `Context` (ADR-008 stable API).
///
/// The *type* is always `espforge_runtime::Delay`; its behaviour is selected by
/// the `embassy` feature so the same `ctx.delay.delay_ms(..)` call site works in
/// both runtimes:
/// - **blocking** (default): wraps `esp_hal::delay::Delay`; `delay_ms` blocks the
///   current thread (v1 style, implements `embedded_hal::delay::DelayNs`).
/// - **embassy** (`feature = "embassy"`): `delay_ms` is `async` and yields via
///   `embassy_time::Timer`, so it must be `.await`ed in `app.rs`. This lets a
///   single `Context` field serve both runtimes without regenerating the struct.
#[derive(Clone, Copy)]
pub struct Delay {
    #[cfg(not(feature = "embassy"))]
    inner: esp_hal::delay::Delay,
}

impl Delay {
    pub fn new() -> Self {
        Delay {
            #[cfg(not(feature = "embassy"))]
            inner: esp_hal::delay::Delay::new(),
        }
    }

    /// Blocking millisecond delay. Present in the default (blocking) build.
    #[cfg(not(feature = "embassy"))]
    pub fn delay_ms(&self, ms: u32) {
        self.inner.delay_millis(ms);
    }

    /// Async millisecond delay. Present only when `feature = "embassy"`; call
    /// with `ctx.delay.delay_ms(ms).await` from an async context (e.g. a spawned
    /// embassy task or the `forever` loop).
    #[cfg(feature = "embassy")]
    pub async fn delay_ms(&self, ms: u32) {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(ms as u64)).await;
    }
}

#[cfg(not(feature = "embassy"))]
impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        self.inner.delay_ns(ns);
    }
}

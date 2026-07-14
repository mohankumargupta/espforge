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

// The `components` namespace is always present; individual capability modules
// are gated per-sub-module in `components/mod.rs` (design §19.1). Gating the
// whole namespace on `any(feature = ...)` here caused the module to be silently
// compiled out (and `espforge_runtime::components::X` to fail to resolve) when a
// new component feature was added but forgotten in the list — so we keep the
// namespace unconditionally compiled and let the per-struct `#[cfg]` gates do
// the real selection. The emitter only ever references a component whose
// feature is enabled, so there is no dead-code cost.
pub mod components;
#[cfg(feature = "ssd1306")]
pub mod devices;

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

/// Delay handle stored on the `Context`. Own type wrapping `esp_hal::delay::Delay`
/// (v1 style), implementing `embedded-hal`'s `DelayNs`.
#[derive(Clone, Copy)]
pub struct Delay {
    inner: esp_hal::delay::Delay,
}

impl Delay {
    pub fn new() -> Self {
        Delay { inner: esp_hal::delay::Delay::new() }
    }
    pub fn delay_ms(&self, ms: u32) {
        self.inner.delay_millis(ms);
    }
}

impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        self.inner.delay_ns(ns);
    }
}

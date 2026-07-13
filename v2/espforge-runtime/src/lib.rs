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

// Capability modules are compiled in only when the generated project enables
// their feature (design §19.1). A `helloworld` project that lists no features
// compiles neither `components` nor `devices`.
#[cfg(any(feature = "led", feature = "i2c", feature = "button"))]
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

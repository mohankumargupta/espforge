//! espforge-runtime: `no_std` runtime implementations of each capability.
//!
//! Unlike esphome, this crate keeps **separate `components` and `devices`
//! modules** (ADR-007), mirroring the three-tier domain spine (ADR-003).
//!
//! This is a target-only crate: the types are concrete `esp-hal` types, so the
//! generated project wires instances move-by-value (ADR-008) with no generics.
//! It depends only on `esp-hal` (+ `embedded-hal` re-exported traits), satisfying
//! the ADR-007 leaf rule.

#![no_std]

use embedded_hal::delay::DelayNs;

pub mod components;
pub mod devices;

/// Minimal logging handle stored on the `Context` (ADR-008 stable API). The
/// generated project initializes the real logger (e.g. `esp_println`) at entry;
/// this type is the app-facing handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Logger;

impl Logger {
    pub fn new() -> Self {
        Logger
    }
    /// Emit a log line. The generated project wires the real sink (e.g.
    /// `esp_println`); this handle is the app-facing API.
    pub fn log(&self, _msg: &str) {}
}

/// Delay handle stored on the `Context`, wrapping `esp_hal::delay::Delay`.
#[derive(Debug, Clone)]
pub struct Delay {
    inner: esp_hal::delay::Delay,
}

impl Delay {
    pub fn new(inner: esp_hal::delay::Delay) -> Self {
        Delay { inner }
    }
    pub fn delay_ms(&mut self, ms: u32) {
        self.inner.delay_ms(ms);
    }
}

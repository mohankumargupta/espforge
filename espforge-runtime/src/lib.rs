//! espforge-runtime: no_std runtime support for generated firmware projects.
//!
//! Target-only in practice, but it compiles on the host too (the esp-hal
//! parts are cfg-gated) so `cargo build`/`cargo test` on the workspace work.

#![cfg_attr(not(test), no_std)]

/// Logging handle stored on the generated `Context`. Forwards to the `log`
/// facade; the generated project installs the sink (esp-println).
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

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

/// Delay handle stored on the generated `Context`.
///
/// - blocking (default): `delay_ms` blocks via `esp_hal::delay::Delay`.
/// - `embassy` feature: `delay_ms` is `async` and yields via `embassy_time`.
#[derive(Clone, Copy)]
pub struct Delay {
    #[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
    inner: esp_hal::delay::Delay,
}

impl Delay {
    pub fn new() -> Self {
        Delay {
            #[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
            inner: esp_hal::delay::Delay::new(),
        }
    }

    #[cfg(all(
        not(feature = "embassy"),
        any(target_arch = "riscv32", target_arch = "xtensa")
    ))]
    pub fn delay_ms(&self, ms: u32) {
        self.inner.delay_millis(ms);
    }

    #[cfg(feature = "embassy")]
    pub async fn delay_ms(&self, ms: u32) {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(ms as u64)).await;
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

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

    /// Busy-waits for `ns` nanoseconds (blocking, targets only). Used by the
    /// `spi` component for the `DelayNs` op inside a transaction; a blocking
    /// wait is fine there (the `DelayNs` ops inside a transaction are
    /// microsecond-scale, so a busy wait is acceptable even under embassy).
    #[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
    pub fn delay_ns(&self, ns: u32) {
        self.inner.delay_nanos(ns);
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

/// The embedded-hal traits, re-exported so generated projects can write
/// transaction operation lists (`embedded_hal::spi::Operation`) through
/// `crate::` without adding the crate as a dependency (ADR-008).
pub use embedded_hal;

/// Hardware-peripheral components (`# components` in the .espf). The inline
/// module makes `components/i2c.rs` resolve as `src/components/i2c.rs`.
/// esp-hal only builds for esp targets, so the module is target-gated.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub mod components {
    pub mod button;
    pub mod i2c;
    pub mod led;
    pub mod spi;
    pub mod uart;
}

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use components::button::Button;

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use components::led::Led;

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use components::i2c::{I2cBus, I2cConfig, I2cError};

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use components::spi::{SpiBus, SpiConfig, SpiDevice, SpiError};

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use components::uart::{UartConfig, UartDevice, UartError};

/// Software services (`# services` in the .espf). The inline module makes
/// `services/wifi.rs` resolve as `src/services/wifi.rs`.
#[cfg(all(feature = "wifi", any(target_arch = "riscv32", target_arch = "xtensa")))]
pub mod services {
    pub mod wifi;
}

#[cfg(all(feature = "wifi", any(target_arch = "riscv32", target_arch = "xtensa")))]
pub use services::wifi::{Wifi, WifiConfig, WifiError};

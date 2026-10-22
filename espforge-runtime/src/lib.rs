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

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use led::Led;

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
mod led {
    use core::cell::RefCell;
    use esp_hal::gpio::{Level, Output, OutputConfig, OutputPin};

    /// An LED on a gpio output. Methods take `&self` because the app reaches
    /// components through `&ctx.components.<name>`; the pin sits in a
    /// `RefCell`. Not `Sync`: use it from the blocking runtime.
    pub struct Led {
        pin: RefCell<Output<'static>>,
        active_low: bool,
    }

    impl Led {
        /// Starts switched off.
        pub fn new(pin: impl OutputPin + 'static, active_low: bool) -> Self {
            let off = if active_low { Level::High } else { Level::Low };
            Self {
                pin: RefCell::new(Output::new(pin, off, OutputConfig::default())),
                active_low,
            }
        }

        pub fn on(&self) {
            let level = if self.active_low {
                Level::Low
            } else {
                Level::High
            };
            self.pin.borrow_mut().set_level(level);
        }

        pub fn off(&self) {
            let level = if self.active_low {
                Level::High
            } else {
                Level::Low
            };
            self.pin.borrow_mut().set_level(level);
        }

        pub fn toggle(&self) {
            self.pin.borrow_mut().toggle();
        }
    }
}

/// Hardware-peripheral components (`# components` in the .espf). The inline
/// module makes `components/i2c.rs` resolve as `src/components/i2c.rs`.
/// Like `led` below, esp-hal only builds for esp targets, so it is
/// target-gated.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub mod components {
    pub mod i2c;
}

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use components::i2c::{I2cBus, I2cConfig, I2cError};

/// Software services (`# services` in the .espf). The inline module makes
/// `services/wifi.rs` resolve as `src/services/wifi.rs`.
#[cfg(all(feature = "wifi", any(target_arch = "riscv32", target_arch = "xtensa")))]
pub mod services {
    pub mod wifi;
}

#[cfg(all(feature = "wifi", any(target_arch = "riscv32", target_arch = "xtensa")))]
pub use services::wifi::{Wifi, WifiConfig, WifiError};

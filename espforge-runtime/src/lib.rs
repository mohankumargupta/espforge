//! espforge-runtime: no_std runtime support for generated firmware projects.
//!
//! The generated entry point builds a [`Logger`] and a [`Delay`], stores them
//! on the project's `Context`, and hands that to the user's `app`. Everything
//! the app touches comes from here, so the app never names `esp-hal` itself.
//!
//! Target-only in practice, but it compiles on the host too (the esp-hal parts
//! live behind one cfg), so `cargo build`/`cargo test` on the workspace work.

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

/// Everything that needs esp-hal. One module, one re-export: adding a runtime
/// type costs a `pub use` here instead of another repeated `cfg` on every
/// call site.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
mod hw {
    use core::cell::RefCell;

    mod i2c;
    mod spi;
    mod uart;
    pub use i2c::*;
    pub use spi::*;
    pub use uart::*;

    /// The Wi-Fi station driver ([`Wifi`], [`WifiConfig`], [`WifiError`]),
    /// behind the `wifi` feature: a project with no radio never compiles it.
    #[cfg(feature = "wifi")]
    mod wifi;
    #[cfg(feature = "wifi")]
    pub use wifi::*;

    use esp_hal::gpio::Level;
    use esp_hal::gpio::{Input, InputConfig, InputPin, Pull};

    /// Delay handle stored on the generated `Context`.
    ///
    /// - blocking (default): `delay_ms` blocks via `esp_hal::delay::Delay`.
    /// - `embassy` feature: `delay_ms` is `async` and yields via `embassy_time`.
    #[derive(Clone, Copy)]
    pub struct Delay {
        inner: esp_hal::delay::Delay,
    }

    impl Delay {
        pub fn new() -> Self {
            Delay {
                inner: esp_hal::delay::Delay::new(),
            }
        }

        #[cfg(not(feature = "embassy"))]
        pub fn delay_ms(&self, ms: u32) {
            self.inner.delay_millis(ms);
        }

        #[cfg(feature = "embassy")]
        pub async fn delay_ms(&self, ms: u32) {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(ms as u64)).await;
        }

        /// Busy-waits for `ns` nanoseconds. Used by the SPI device for the
        /// `DelayNs` op inside a transaction; those waits are microsecond
        /// scale, so a busy wait is fine even under embassy.
        pub fn delay_ns(&self, ns: u32) {
            self.inner.delay_nanos(ns);
        }
    }

    impl Default for Delay {
        fn default() -> Self {
            Self::new()
        }
    }

    /// An LED on a GPIO output.
    ///
    /// The `Output` is held behind a `RefCell` so the mutating methods take
    /// `&self`: the blocking `component!(ctx, led).toggle()` call site keeps
    /// working, and an embassy task can toggle a shared LED.
    pub struct Led {
        pin: RefCell<esp_hal::gpio::Output<'static>>,
        active_low: bool,
    }

    impl Led {
        /// `pin` is the output pin moved in by value. The LED starts *off*:
        /// an active-low LED is off while the pin reads high.
        pub fn new(pin: impl esp_hal::gpio::OutputPin + 'static, active_low: bool) -> Self {
            use esp_hal::gpio::{Output, OutputConfig};
            let off = if active_low { Level::High } else { Level::Low };
            Led {
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

    /// `button` component: a single GPIO input with an optional pull-up, wired
    /// move-by-value (ADR-008).
    ///
    /// The `Input` is held behind a `RefCell` so the mutating/awaiting methods
    /// take `&self` (mirroring [`Led`]). This lets an embassy task hold a shared
    /// `&'static Context` and call `wait_for_pressed().await` (and `is_pressed()`)
    /// without needing a `&mut` borrow of the context — the same reason `Led`
    /// uses interior mutability so a spawned task can `toggle(&self)`.
    pub struct Button {
        pin: RefCell<Input<'static>>,
        pull_up: bool,
    }

    impl Button {
        /// `pin` is the input pin moved in by value. `pull_up` selects the
        /// internal pull resistor: a pulled-up button reads low when pressed
        /// (the pin wired to ground through the switch).
        pub fn new(pin: impl InputPin + 'static, pull_up: bool) -> Self {
            let pull = if pull_up { Pull::Up } else { Pull::None };
            Button {
                pin: RefCell::new(Input::new(pin, InputConfig::default().with_pull(pull))),
                pull_up,
            }
        }

        /// Current pressed state. Pulled-up buttons read low when pressed.
        pub fn is_pressed(&self) -> bool {
            if self.pull_up {
                self.pin.borrow().is_low()
            } else {
                self.pin.borrow().is_high()
            }
        }

        /// Asynchronously wait until the button is pressed (edge-triggered, not
        /// polled). Pulled-up buttons read low when pressed, so we wait for low;
        /// otherwise we wait for high. Requires the `embassy` feature so the
        /// shared context can be borrowed by a spawned task while waiting.
        #[cfg(feature = "embassy")]
        pub async fn wait_for_pressed(&self) {
            if self.pull_up {
                self.pin.borrow_mut().wait_for_low().await;
            } else {
                self.pin.borrow_mut().wait_for_high().await;
            }
        }
    }

    /// The embedded-hal traits, re-exported so generated projects can name
    /// `embedded_hal::...` through their own crate without adding the
    /// dependency themselves.
    pub use embedded_hal;
}

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use hw::*;

/// The TLS client allocates its record buffers from the heap (the radio
/// already installed one: `wifi` requires the `alloc` option).
#[cfg(feature = "https-client")]
extern crate alloc;

/// Network services: the outgoing HTTP/HTTPS, WebSocket and NTP clients.
/// Gated on the target like `hw`, and on the client features so a project
/// with no service never pulls in `edge-http`, `edge-ws`, `embedded-tls` or
/// `sntpc`.
///
/// Also built under `test`, so the target-free pieces (the URL splitting and
/// the WebSocket vocabulary) have host unit tests.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa", test))]
#[cfg(any(feature = "http-client", feature = "ws-client", feature = "ntp"))]
mod services;
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa", test))]
#[cfg(any(feature = "http-client", feature = "ws-client", feature = "ntp"))]
pub use services::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logger_is_a_cheap_handle() {
        let logger = Logger::new();
        let copy = logger;
        logger.info("hello");
        copy.warn("still usable");
        let _ = Logger::default();
    }
}

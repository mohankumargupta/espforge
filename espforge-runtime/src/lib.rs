//! espforge-runtime: no_std runtime support for generated firmware projects.
//!
//! The generated entry point builds a [`Logger`] and a [`Delay`], stores them
//! on the project's `Context`, and hands that to the user's `app`. Everything
//! the app touches comes from here, so the app never names `esp-hal` itself.
//!
//! Target-only in practice, but it compiles on the host too (the esp-hal parts
//! live behind one cfg), so `cargo build`/`cargo test` on the workspace work.
//!
//! # Per-instance storage
//!
//! A singleton never hides its storage in a `static`: `I2cBus::build_in`,
//! `SpiBus::build_in`/`build_write_only_in`, `Wifi::start_in` and
//! `HttpClient::new_in` take a `&'static` storage value (`I2cBusStorage`,
//! `SpiBusStorage`, `WifiStorage`, `HttpClientStorage`) that the generated code
//! declares once per element. Two buses, two radios or two clients therefore
//! cannot share a cell, and a cell can never be initialised twice because no
//! element's storage is reused. The older constructors (`I2cBus::build`,
//! `SpiBus::build`, `Wifi::start`, `HttpClient::new`) are gone: a project that
//! pins an earlier runtime must be regenerated to get the `*_in` call sites.

#![cfg_attr(not(test), no_std)]

/// Wall time and the NTP era. Host-testable: the arithmetic names no chip API,
/// and the `Clock` handle only appears once the `embassy` feature is on.
pub mod clock;
pub use clock::*;

/// Deferred device construction: build the handle at boot, initialise the part
/// after `app::setup`. Host-testable: a factory is plain Rust, and only the
/// async half needs the `embassy` feature.
mod deferred;
pub use deferred::*;

/// The target-aware logger `<logger>` configures.
///
/// The generated code owns the one logger `static` (and the rule table), so
/// this module adds no storage of its own: `espforge_runtime::FilteredLogger`
/// is a value, and [`init`] installs the reference the project declared.
pub mod logger;
pub use logger::{FilterRule, FilteredLogger, init as init_logger};

/// Logging handle stored on the generated `Context`. Forwards to the `log`
/// facade; the generated project installs the sink (esp-println, directly or
/// through [`FilteredLogger`]).
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

/// Why a device could not be constructed.
///
/// A driver returns this, not a panic: one loose jumper is a named, logged
/// failure instead of an infinite boot loop before `app::setup()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceErrorKind {
    /// The part did not answer, or rejected its initialization sequence.
    Init,
    /// The bus rejected a transaction while the device was being set up.
    Bus,
}

impl core::fmt::Display for DeviceErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DeviceErrorKind::Init => f.write_str("initialization failed"),
            DeviceErrorKind::Bus => f.write_str("bus setup failed"),
        }
    }
}

/// A device that failed to construct, named by its `.espf` id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceError {
    /// The element id as written in the project file (`oled`).
    pub device: &'static str,
    /// Why the driver refused.
    pub kind: DeviceErrorKind,
}

impl core::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "device `{}`: {}", self.device, self.kind)
    }
}

/// Everything that needs esp-hal. One module, one re-export: adding a runtime
/// type costs a `pub use` here instead of another repeated `cfg` on every
/// call site.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
mod hw {
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

    /// Dual-slot A/B OTA updates ([`Ota`], [`OtaStorage`], [`OtaError`]),
    /// behind the `ota` feature: a project without `<ota />` never compiles
    /// `esp-hal-ota` or touches flash.
    #[cfg(feature = "ota")]
    mod ota;
    #[cfg(feature = "ota")]
    pub use ota::*;

    use esp_hal::gpio::Level;
    use esp_hal::gpio::{Flex, Input, InputConfig, InputPin, Pin, Pull};

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

    /// The blocking wait `embedded-hal-bus`'s SPI adapters take, so a
    /// `RefCellDevice` can honour a transaction's `DelayNs` operation.
    impl embedded_hal::delay::DelayNs for Delay {
        fn delay_ns(&mut self, ns: u32) {
            self.inner.delay_nanos(ns);
        }
    }

    /// The standard bus-sharing adapters, re-exported so generated code names
    /// them through this crate and a project never adds the dependency itself.
    pub use embedded_hal_bus;

    /// An LED on a GPIO output.
    ///
    /// Interior mutability lives in the container, not here: on a blocking
    /// board the app holds `&mut Led`, and on an embassy board it holds an
    /// `AsyncMutexGuard<'_, Led>`. Both reach these `&mut self` methods.
    pub struct Led {
        pin: esp_hal::gpio::Output<'static>,
        active_low: bool,
    }

    impl Led {
        /// `pin` is the output pin moved in by value. The LED starts *off*:
        /// an active-low LED is off while the pin reads high.
        pub fn new(pin: impl esp_hal::gpio::OutputPin + 'static, active_low: bool) -> Self {
            use esp_hal::gpio::{Output, OutputConfig};
            let off = if active_low { Level::High } else { Level::Low };
            Led {
                pin: Output::new(pin, off, OutputConfig::default()),
                active_low,
            }
        }

        pub fn on(&mut self) {
            let level = if self.active_low {
                Level::Low
            } else {
                Level::High
            };
            self.pin.set_level(level);
        }

        pub fn off(&mut self) {
            let level = if self.active_low {
                Level::High
            } else {
                Level::Low
            };
            self.pin.set_level(level);
        }

        pub fn toggle(&mut self) {
            self.pin.toggle();
        }
    }

    /// `button` device: a single GPIO input with an optional pull-up, wired
    /// move-by-value (ADR-008).
    ///
    /// Like [`Led`], the pin is a plain field: the device's container supplies
    /// the mutability (`&mut Button` on blocking, an `AsyncMutexGuard` on
    /// embassy), so a spawned task reaches `wait_for_pressed` through its
    /// guard rather than an inner `RefCell`.
    pub struct Button {
        pin: Input<'static>,
        pull_up: bool,
    }

    impl Button {
        /// `pin` is the input pin moved in by value. `pull_up` selects the
        /// internal pull resistor: a pulled-up button reads low when pressed
        /// (the pin wired to ground through the switch).
        pub fn new(pin: impl InputPin + 'static, pull_up: bool) -> Self {
            let pull = if pull_up { Pull::Up } else { Pull::None };
            Button {
                pin: Input::new(pin, InputConfig::default().with_pull(pull)),
                pull_up,
            }
        }

        /// Current pressed state. Pulled-up buttons read low when pressed.
        pub fn is_pressed(&mut self) -> bool {
            if self.pull_up {
                self.pin.is_low()
            } else {
                self.pin.is_high()
            }
        }

        /// Asynchronously wait until the button is pressed (edge-triggered, not
        /// polled). Pulled-up buttons read low when pressed, so we wait for low;
        /// otherwise we wait for high. Requires the `embassy` feature so the
        /// shared context can be borrowed by a spawned task while waiting.
        #[cfg(feature = "embassy")]
        pub async fn wait_for_pressed(&mut self) {
            if self.pull_up {
                self.pin.wait_for_low().await;
            } else {
                self.pin.wait_for_high().await;
            }
        }
    }

    /// A GPIO line the app both drives and reads.
    ///
    /// A one-wire part (a DHT11/DHT22, a DS18B20) shares one line: the app
    /// pulls it low to start a transfer, then releases it so the pull-up — or
    /// the part — sets the level it reads. `esp-hal`'s `Flex` is the pin, and
    /// this type keeps the two states explicit so a driver cannot leave a line
    /// it drove still driven.
    ///
    /// The line starts released: nothing drives it until the driver asks.
    pub struct Bidirectional {
        pin: Flex<'static>,
    }

    impl Bidirectional {
        /// `pin` is the line moved in by value; `pull` is the resistor it reads
        /// through while released.
        pub fn new(pin: impl Pin + 'static, pull: Pull) -> Self {
            let mut pin = Flex::new(pin);
            pin.apply_input_config(&InputConfig::default().with_pull(pull));
            pin.set_input_enable(true);
            pin.set_output_enable(false);
            Self { pin }
        }

        /// Drive the line as an output at `level`.
        pub fn drive(&mut self, level: Level) {
            self.pin.set_level(level);
            self.pin.set_output_enable(true);
        }

        /// Release the line: the output driver off, so the pull resistor sets
        /// the level other devices and this one read.
        pub fn release(&mut self) {
            self.pin.set_output_enable(false);
        }

        /// The level the line reads.
        pub fn level(&self) -> Level {
            self.pin.level()
        }

        /// Whether the line reads high.
        pub fn is_high(&self) -> bool {
            self.pin.is_high()
        }

        /// Whether the line reads low.
        pub fn is_low(&self) -> bool {
            self.pin.is_low()
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

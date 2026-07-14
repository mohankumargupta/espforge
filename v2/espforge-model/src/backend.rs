//! Codegen backends (ADR-008, embassy-ready).
//!
//! A backend renders the *backend-specific* parts of a constructed instance —
//! how a GPIO pin becomes an `Output`, how an I2C peripheral becomes a bus, and
//! how a runtime constructor call is written. Drivers stay backend-agnostic:
//! they resolve pin numbers / polarities / borrowed components, then ask the
//! backend to render. Adding embassy is a new `Backend` impl, not a rewrite of
//! every driver (which is why this is a trait, not a match on `ctx.is_embassy`).

use crate::ir::Tier;

/// Renders backend-specific construction snippets for drivers (ADR-008).
pub trait Backend: Debug + Send + Sync {
    /// The path to this backend's runtime types, e.g. `espforge_runtime`. Used
    /// to namespace emitted constructor calls.
    fn runtime_path(&self) -> &str;

    /// Wrap a moved-in GPIO peripheral into an `Output`, polarity-aware. The
    /// `active_low` flag selects the idle level (ADR-003).
    fn gpio_output(&self, gpio: &str, active_low: bool) -> String;

    /// Wrap a moved-in GPIO peripheral into an `Input` with a pull resistor.
    /// `pull_up` selects `Pull::Up`; otherwise `Pull::None` (ADR-003).
    fn gpio_input(&self, gpio: &str, pull_up: bool) -> String;

    /// Construct an I2C master bus from its peripheral + sda/scl pins.
    fn i2c_master(&self, i2c: &str, sda: &str, scl: &str) -> String;

    /// Construct an SPI master bus from its peripheral + mosi/miso/sclk/cs pins
    /// and mode/frequency. `cs` is attached to the master so transfers manage
    /// the chip-select line automatically.
    fn spi_master(
        &self,
        spi: &str,
        mosi: &str,
        miso: &str,
        sclk: &str,
        cs: &str,
        mode: u8,
        frequency_khz: u32,
    ) -> String;

    /// Construct a UART from its peripheral + tx/rx pins and baud rate.
    fn uart(&self, uart: &str, tx: &str, rx: &str, baud: u32) -> String;

    /// Render a call to a runtime constructor, e.g.
    /// `espforge_runtime::components::Led::new(a, b)`.
    fn ctor(&self, tier: Tier, type_name: &str, args: &[String]) -> String;
}

use std::fmt::Debug;

/// The blocking backend (default). Mirrors the `esp_hal` blocking API. The
/// embassy backend is added later as a sibling impl — no driver changes needed.
#[derive(Debug, Default)]
pub struct Blocking;

impl Backend for Blocking {
    fn runtime_path(&self) -> &str {
        "espforge_runtime"
    }

    fn gpio_output(&self, gpio: &str, active_low: bool) -> String {
        let level = if active_low { "High" } else { "Low" };
        format!(
            "esp_hal::gpio::Output::new(registry.peripherals.{gpio}, esp_hal::gpio::Level::{level}, esp_hal::gpio::OutputConfig::default())"
        )
    }

    fn gpio_input(&self, gpio: &str, pull_up: bool) -> String {
        let pull = if pull_up { "Up" } else { "None" };
        format!(
            "esp_hal::gpio::Input::new(registry.peripherals.{gpio}, esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::{pull}))"
        )
    }

    fn i2c_master(&self, i2c: &str, sda: &str, scl: &str) -> String {
        format!(
            "espforge_runtime::components::I2cBus::new(registry.peripherals.{i2c}, registry.peripherals.GPIO{sda}, registry.peripherals.GPIO{scl})"
        )
    }

    fn spi_master(
        &self,
        spi: &str,
        mosi: &str,
        miso: &str,
        sclk: &str,
        cs: &str,
        mode: u8,
        frequency_khz: u32,
    ) -> String {
        format!(
            "espforge_runtime::components::SpiBus::new(\
                 registry.peripherals.{spi}, \
                 registry.peripherals.GPIO{mosi}, \
                 registry.peripherals.GPIO{miso}, \
                 registry.peripherals.GPIO{sclk}, \
                 registry.peripherals.GPIO{cs}, \
                 {mode}, \
                 {frequency_khz}\
             )"
        )
    }

    fn uart(&self, uart: &str, tx: &str, rx: &str, baud: u32) -> String {
        format!(
            "espforge_runtime::components::UartDevice::new(\
                 registry.peripherals.{uart}, \
                 registry.peripherals.GPIO{tx}, \
                 registry.peripherals.GPIO{rx}, \
                 {baud}\
             )"
        )
    }

    fn ctor(&self, tier: Tier, type_name: &str, args: &[String]) -> String {
        let module = match tier {
            Tier::Component => "components",
            Tier::Device => "devices",
        };
        format!(
            "{rt}::{module}::{ty}::new({args})",
            rt = self.runtime_path(),
            module = module,
            ty = type_name,
            args = args.join(", ")
        )
    }
}

/// The process-wide default backend. A single `&'static` instance keeps
/// `GenContext` cheap and lets `FnDriver` (test) contexts share it.
pub static BLOCKING: Blocking = Blocking;

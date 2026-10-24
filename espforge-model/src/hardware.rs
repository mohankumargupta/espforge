//! Hardware and service specs: the domain model `build` can generate code for.
//!
//! Moved verbatim from `espforge-emit/src/context.rs` (step 1b): no logic
//! change, only the home. `espforge-emit::context` keeps the minijinja
//! `Value` mapping; everything else imports these types from here.

/// One `<led>` component, already resolved to a gpio pin number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedSpec {
    /// Rust identifier: the component id with `-` turned into `_`.
    pub name: String,
    pub pin: u8,
    pub active_low: bool,
}

/// One `<button>` component, already resolved to a gpio pin number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonSpec {
    /// Rust identifier: the component id with `-` turned into `_`.
    pub name: String,
    pub pin: u8,
    pub pull_up: bool,
}

/// `<i2c>` in `# peripherals`, plus the `<i2c_component>` on it, resolved to
/// pins and a bus frequency. Both become one Rust field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cSpec {
    /// Rust identifier: the component id with `-` turned into `_`.
    pub name: String,
    /// Upper-cased esp-hal peripheral name (`I2C0`, `I2C1`, ...).
    pub peripheral: String,
    pub sda: u8,
    pub scl: u8,
    /// Bus frequency in kHz.
    pub khz: u32,
}

/// A `<spi>` peripheral that at least one `<spi_component>` rides on.
/// Diagnostic/validation use the raw id; the template uses the fields to
/// emit the bus setup once per peripheral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiBusSpec {
    /// The `<spi>` id (`spi2`); generated code names the bus handle `{id}_bus`.
    pub id: String,
    /// Upper-cased esp-hal peripheral name (`SPI2`).
    pub peripheral: String,
    pub sclk: u8,
    pub mosi: u8,
    /// `None` is a write-only bus (displays): the generated code calls
    /// `SpiBus::build_write_only` and attaches no MISO pin.
    pub miso: Option<u8>,
    /// Bus frequency in kHz. Bus clock: the one `SpiBus` carries.
    pub khz: u32,
    /// SPI mode 0..=3 (`Mode::_0`..`Mode::_3` in the template).
    pub mode: u8,
}

/// One `<spi_component>`: a device with its own chip-select on a shared
/// `<spi>` bus (the bus is `SpiSpec::bus`, a `SpiBusSpec::id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiSpec {
    /// Rust identifier: the component id with `-` turned into `_`.
    pub name: String,
    /// The `<spi>` bus id this device rides on.
    pub bus: String,
    /// Device-local chip-select pin (always owned by one component).
    pub cs: u8,
}

/// One `<uart_component>`: a UART peripheral with its pins and framing,
/// resolved from the component that rides on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UartSpec {
    /// Rust identifier: the component id with `-` turned into `_`.
    pub name: String,
    /// Upper-cased esp-hal peripheral name (`UART0`, `UART1`, ...).
    pub peripheral: String,
    pub tx: u8,
    pub rx: u8,
    /// Baud rate the component sets.
    pub baud: u32,
    /// Template token for `<uart_component parity=..>`: `"None"`, `"Even"` or
    /// `"Odd"` (used as `esp_hal::uart::Parity::{{ u.parity }}`).
    pub parity: String,
    /// Template token for `<uart_component stop_bits=..>`: `"_1"` or `"_2"`
    /// (used as `esp_hal::uart::StopBits::{{ u.stop_bits }}`).
    pub stop_bits: String,
}

/// The `<wifi>` service. Both strings are written into Rust string literals,
/// so the caller must have rejected `\\`, `"` and line breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiSpec {
    pub ssid: String,
    /// Empty means an open network.
    pub password: String,
}

/// The hardware and services the generated project has to construct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardware {
    pub leds: Vec<LedSpec>,
    pub buttons: Vec<ButtonSpec>,
    pub i2cs: Vec<I2cSpec>,
    /// Used bus peripherals first (template order), then devices.
    pub spi_buses: Vec<SpiBusSpec>,
    pub spis: Vec<SpiSpec>,
    pub uarts: Vec<UartSpec>,
    pub wifi: Option<WifiSpec>,
}

/// Whether `s` can be used as a Rust identifier (letters, digits, `_`; not
/// starting with a digit). Component ids are slugified with `-` -> `_`
/// before this check.
pub fn is_rust_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

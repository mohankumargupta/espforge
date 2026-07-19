//! `spi` component: an SPI master bus (ADR-003/008 bus-sharing).
//!
//! Parametric over the esp-hal mode typestate `Dm` (design §20.2). `SpiBus<Dm>`
//! is the shared bus; `SpiDevice<Dm>` is one device on that bus that owns its
//! own chip-select pin. **CS lives in `SpiDevice`, never on the bus** — the old
//! bus-level `cs: Option<Output>` parameter is gone (§20.5); multiple devices
//! sharing one bus each lock the shared bus mutex but assert their own CS.
//!
//! Interior mutability keyed on `Dm` (§20.3): blocking bus uses `RefCell`; async
//! bus uses `embassy_sync::Mutex<RefCell<..>>` for safe cross-task sharing.
//!
//! `SpiDevice` implements `embedded_hal::spi::SpiDevice` (and the async variant
//! under `embassy`) so it can be passed directly to SPI drivers.

use core::cell::RefCell;
use esp_hal::gpio::{InputPin, Output, OutputPin};
use esp_hal::spi::master::{Config as EspConfig, Spi};
use esp_hal::spi::Mode;
#[cfg(not(feature = "embassy"))]
use esp_hal::Blocking;
use esp_hal::DriverMode;
#[cfg(feature = "embassy")]
use esp_hal::Async;
use esp_hal::peripherals::SPI2;

use crate::Delay;

/// Minimal YAML-facing SPI config (design §20.6, Level B). `frequency` +
/// `mode` only; bit-order defaults. esp-hal's full `Config` also carries an
/// unstable `clock_source` field we don't expose.
#[derive(Debug, Clone, Copy)]
pub struct SpiConfig {
    pub frequency: esp_hal::time::Rate,
    pub mode: Mode,
}

impl Default for SpiConfig {
    fn default() -> Self {
        SpiConfig {
            frequency: esp_hal::time::Rate::from_mhz(1),
            mode: Mode::_0,
        }
    }
}

impl From<SpiConfig> for EspConfig {
    fn from(c: SpiConfig) -> EspConfig {
        EspConfig::default()
            .with_frequency(c.frequency)
            .with_mode(c.mode)
    }
}

/// Unified SPI error surfaced to examples. (`ConfigError` lacks `Eq`, and the
/// esp-hal bus `Error` is private, so `Bus` carries no payload — examples only
/// ever match on `Err(_)`.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpiError {
    Config(esp_hal::spi::master::ConfigError),
    Bus,
}

impl embedded_hal::spi::Error for SpiError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

// ---------------------------------------------------------------------------
// One parametric `SpiBus` struct; inner sharing primitive differs by build.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
pub struct SpiBus<Dm: DriverMode + 'static> {
    inner: &'static RefCell<Spi<'static, Dm>>,
}

#[cfg(feature = "embassy")]
pub struct SpiBus<Dm: DriverMode + 'static> {
    inner: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        RefCell<Spi<'static, Dm>>,
    >,
}

// ---------------------------------------------------------------------------
// Blocking build
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl SpiBus<Blocking> {
    /// Build the owned esp-hal `Spi` master, park it in a `static` cell, return
    /// a `Copy` `SpiBus` handle. esp-hal 1.1: `Spi::new(spi, config)` takes the
    /// peripheral + config; pins attached via `with_sck`/`with_mosi`/`with_miso`;
    /// **no `clocks` arg, no bus-level CS** (§20.1/§20.5). Fallible per §20.7.
    pub fn build(
        spi: SPI2<'static>,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus<Blocking>, esp_hal::spi::master::ConfigError> {
        static CELL: static_cell::StaticCell<RefCell<Spi<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso);
        let inner = CELL.init(RefCell::new(esp));
        Ok(SpiBus { inner })
    }
}

// ---------------------------------------------------------------------------
// Async build (only under `embassy`)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
impl SpiBus<Async> {
    pub fn build(
        spi: SPI2<'static>,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus<Async>, esp_hal::spi::master::ConfigError> {
        static CELL: static_cell::StaticCell<
            embassy_sync::mutex::Mutex<
                embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
                RefCell<Spi<'static, Async>>,
            >,
        > = static_cell::StaticCell::new();
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso)
            .into_async();
        let inner = CELL.init(embassy_sync::mutex::Mutex::new(RefCell::new(esp)));
        Ok(SpiBus { inner })
    }
}

// ---------------------------------------------------------------------------
// Device (owns CS) — both modes
// ---------------------------------------------------------------------------

// `cs`/`delay` need interior mutability so `transaction` can take `&self` —
// espforge hands components to apps as a shared `&` (or `&'static`) ref via the
// `component!` macro, so we can never demand `&mut` (design §20.3/ADR-008).
// `RefCell` is enough in both modes: a given `SpiDevice` is serialised by the
// bus `Mutex` (async) / by single-threaded execution (blocking), so `cs`/
// `delay` are never touched concurrently with each other.
type CsCell = RefCell<Output<'static>>;
type DelayCell = RefCell<Delay>;

pub struct SpiDevice<Dm: DriverMode + 'static> {
    bus: SpiBus<Dm>,
    cs: CsCell,
    delay: DelayCell,
}

impl<Dm: DriverMode + 'static> SpiDevice<Dm> {
    /// Attach a device with its own CS pin to an already-built bus. The bus is
    /// shared; the CS is device-local (§20.5). `delay` is used for the `DelayNs`
    /// op inside a transaction.
    pub fn new(bus: SpiBus<Dm>, cs: Output<'static>, delay: Delay) -> Self {
        SpiDevice {
            bus,
            cs: RefCell::new(cs),
            delay: RefCell::new(delay),
        }
    }

    pub fn delay_clone(&self) -> Delay {
        self.delay.borrow().clone()
    }
}

/// Run the operations against a `&mut Spi`. Generic over `Dm` so one body
/// serves blocking and async (esp-hal implements `embedded_hal::spi::SpiBus`
/// for `Spi<'_>` in every `Dm`, §20.5).
fn run_ops<Dm: DriverMode + 'static>(
    bus: &mut Spi<'static, Dm>,
    cs: &mut Output<'static>,
    delay: &mut Delay,
    operations: &mut [embedded_hal::spi::Operation<'_, u8>],
) -> Result<(), SpiError> {
    use embedded_hal::spi::{Operation, SpiBus};
    cs.set_low();
    let result = (|| {
        for op in operations.iter_mut() {
            match op {
                Operation::Read(buf) => bus.read(buf).map_err(|_| SpiError::Bus)?,
                Operation::Write(buf) => bus.write(buf).map_err(|_| SpiError::Bus)?,
                // esp-hal's inherent `transfer` is in-place (1 buffer) and shadows
                // the trait, so reach the two-buffer full-duplex method via UFCS.
                Operation::Transfer(read, write) => {
                    <Spi<'_, Dm> as SpiBus>::transfer(bus, read, write)
                        .map_err(|_| SpiError::Bus)?
                }
                Operation::TransferInPlace(buf) => {
                    bus.transfer_in_place(buf).map_err(|_| SpiError::Bus)?
                }
                Operation::DelayNs(ns) => {
                    delay.delay_ns((*ns).try_into().unwrap_or(u32::MAX));
                }
            }
        }
        Ok(())
    })();
    cs.set_high();
    result
}

/// Inherent `transaction` so apps can drive SPI through the shared `&`/`&'static`
/// `Context` (the `component!` macro never yields `&mut`). Mirrors the trait
/// method but takes `&self` via interior mutability on `cs`/`delay` (§20.3).
#[cfg(not(feature = "embassy"))]
impl SpiDevice<Blocking> {
    pub fn transaction(
        &self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        let mut bus = self.bus.inner.borrow_mut();
        let mut cs = self.cs.borrow_mut();
        let mut delay = self.delay.borrow_mut();
        run_ops(&mut *bus, &mut *cs, &mut *delay, operations)
    }
}

#[cfg(feature = "embassy")]
impl SpiDevice<Async> {
    pub async fn transaction(
        &self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        let bus_guard = self.bus.inner.lock().await;
        let mut bus = bus_guard.borrow_mut();
        let mut cs_guard = self.cs.borrow_mut();
        let mut delay_guard = self.delay.borrow_mut();
        run_ops(&mut *bus, &mut *cs_guard, &mut *delay_guard, operations)
    }
}

#[cfg(not(feature = "embassy"))]
impl embedded_hal::spi::ErrorType for SpiDevice<Blocking> {
    type Error = SpiError;
}

#[cfg(feature = "embassy")]
impl embedded_hal::spi::ErrorType for SpiDevice<Async> {
    type Error = SpiError;
}

#[cfg(not(feature = "embassy"))]
impl embedded_hal::spi::SpiDevice for SpiDevice<Blocking> {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        SpiDevice::transaction(self, operations)
    }
}

#[cfg(feature = "embassy")]
impl embedded_hal_async::spi::SpiDevice for SpiDevice<Async> {
    async fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        SpiDevice::transaction(self, operations).await
    }
}

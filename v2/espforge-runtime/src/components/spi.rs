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
use esp_hal::spi::master::{Config as EspConfig, Error as EspError, Spi};
use esp_hal::spi::Mode;
use esp_hal::{Blocking, DriverMode};
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

/// Unified SPI error surfaced to examples. (`ConfigError` lacks `Eq`.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpiError {
    Config(esp_hal::spi::master::ConfigError),
    Bus(EspError),
}

impl embedded_hal::spi::Error for SpiError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

// ---------------------------------------------------------------------------
// Blocking bus
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SpiBus<Dm: DriverMode + 'static> {
    inner: &'static RefCell<Spi<'static, Dm>>,
}

impl SpiBus<Blocking> {
    /// Build the owned esp-hal `Spi` master, park it in a `static` cell, and
    /// return a `Copy` `SpiBus` handle. esp-hal 1.1: `new(config)` only — pins
    /// attached via `with_sck`/`with_mosi`/`with_miso`; **no `clocks` arg, no
    /// bus-level CS** (§20.1/§20.5). Fallible per §20.7.
    pub fn build(
        spi: SPI2<'static>,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus<Blocking>, esp_hal::spi::master::ConfigError> {
        static CELL: static_cell::StaticCell<RefCell<Spi<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = Spi::new(EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso);
        let inner = CELL.init(RefCell::new(esp));
        Ok(SpiBus { inner })
    }

    #[cfg(feature = "embassy")]
    pub fn into_async(self) -> SpiBus<Async> {
        static CELL: static_cell::StaticCell<
            embassy_sync::mutex::Mutex<RefCell<Spi<'static, Async>>>,
        > = static_cell::StaticCell::new();
        let esp = self.inner.borrow_mut().into_async();
        let inner = CELL.init(embassy_sync::mutex::Mutex::new(RefCell::new(esp)));
        SpiBus { inner }
    }
}

// ---------------------------------------------------------------------------
// Async bus (only under `embassy`, §20.3)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
pub struct SpiBus<Async> {
    inner: &'static embassy_sync::mutex::Mutex<RefCell<Spi<'static, Async>>>,
}

#[cfg(feature = "embassy")]
impl SpiBus<Async> {
    pub fn into_blocking(self) -> SpiBus<Blocking> {
        static CELL: static_cell::StaticCell<RefCell<Spi<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = self.inner.into_inner().into_blocking();
        let inner = CELL.init(RefCell::new(esp));
        SpiBus { inner }
    }
}

// ---------------------------------------------------------------------------
// Device (owns CS) — both modes
// ---------------------------------------------------------------------------

pub struct SpiDevice<Dm: DriverMode + 'static> {
    bus: SpiBus<Dm>,
    cs: Output<'static>,
    delay: Delay,
}

impl<Dm: DriverMode + 'static> SpiDevice<Dm> {
    /// Attach a device with its own CS pin to an already-built bus. The bus is
    /// shared; the CS is device-local (§20.5). `delay` is `Copy` (§12) and is
    /// used for the `DelayNs` op inside a transaction.
    pub fn new(bus: SpiBus<Dm>, cs: Output<'static>, delay: Delay) -> Self {
        SpiDevice { bus, cs, delay }
    }

    pub fn delay_clone(&self) -> Delay {
        self.delay
    }
}

#[cfg(feature = "embassy")]
impl SpiDevice<Blocking> {
    /// Morph the device's bus to async (design §20.1).
    pub fn into_async(self) -> SpiDevice<Async> {
        SpiDevice {
            bus: self.bus.into_async(),
            cs: self.cs,
            delay: self.delay,
        }
    }
}

impl embedded_hal::spi::ErrorType for SpiDevice<Blocking> {
    type Error = SpiError;
}

/// Run the operations against a `&mut Spi` (works for both modes: esp-hal
/// implements `embedded_hal::spi::SpiBus` for `Spi<'_>` in every `Dm`, §20.5).
/// Generic over `Dm` so one body serves blocking and async.
fn run_ops<Dm: esp_hal::DriverMode + 'static>(
    bus: &mut Spi<'static, Dm>,
    cs: &mut Output<'static>,
    delay: &Delay,
    operations: &mut [embedded_hal::spi::Operation<'_, u8>],
) -> Result<(), SpiError> {
    use embedded_hal::delay::DelayNs;
    use embedded_hal::spi::SpiBus as _;
    use embedded_hal::spi::Operation;
    cs.set_low();
    let result = (|| {
        for op in operations.iter_mut() {
            match op {
                Operation::Read(buf) => bus.read(buf).map_err(SpiError::Bus)?,
                Operation::Write(buf) => bus.write(buf).map_err(SpiError::Bus)?,
                Operation::Transfer(read, write) => {
                    bus.transfer(read, write).map_err(SpiError::Bus)?
                }
                Operation::TransferInPlace(buf) => {
                    bus.transfer_in_place(buf).map_err(SpiError::Bus)?
                }
                Operation::DelayNs(ns) => {
                    // esp-hal `DelayNs` takes `u32`; clamp a larger value.
                    delay.delay_ns((*ns).try_into().unwrap_or(u32::MAX));
                }
            }
        }
        Ok(())
    })();
    cs.set_high();
    result
}

impl SpiDevice<Blocking> {
    pub fn transaction(
        &self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        let mut bus = self.bus.inner.borrow_mut();
        run_ops(&mut *bus, &mut self.cs.clone(), &self.delay, operations)
    }
}

#[cfg(feature = "embassy")]
impl SpiDevice<Async> {
    pub async fn transaction(
        &self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        let mut guard = self.bus.inner.lock().await;
        run_ops(&mut *guard.borrow_mut(), &mut self.cs.clone(), &self.delay, operations)
    }
}

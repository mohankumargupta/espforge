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
use esp_hal::mode::{Async, Blocking};
use esp_hal::spi::master::{Config as EspConfig, Error as EspError, Spi};
use esp_hal::spi::Mode;
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
            frequency: EspConfig::default().frequency,
            mode: EspConfig::default().mode,
        }
    }
}

impl From<SpiConfig> for EspConfig {
    fn from(c: SpiConfig) -> EspConfig {
        EspConfig {
            frequency: c.frequency,
            mode: c.mode,
            ..EspConfig::default()
        }
    }
}

/// Unified SPI error surfaced to examples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    Config(esp_hal::spi::master::ConfigError),
    Bus(EspError),
}

// ---------------------------------------------------------------------------
// Blocking bus
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SpiBus<Blocking> {
    inner: &'static RefCell<Spi<'static, Blocking>>,
}

impl SpiBus<Blocking> {
    pub fn from_ref(bus: &'static RefCell<Spi<'static, Blocking>>) -> Self {
        SpiBus { inner: bus }
    }

    /// Build the owned esp-hal `Spi` master. esp-hal 1.1: `new(config)` only —
    /// pins attached via `with_sck`/`with_mosi`/`with_miso`; **no `clocks` arg,
    /// no bus-level CS** (§20.1/§20.5). Fallible per §20.7.
    pub fn build(
        spi: SPI2<'static>,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<&'static RefCell<Spi<'static, Blocking>>, esp_hal::spi::master::ConfigError> {
        let esp = Spi::new(EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso);
        Ok(SpiBus::<Blocking>::leak(esp))
    }

    fn leak(esp: Spi<'static, Blocking>) -> &'static RefCell<Spi<'static, Blocking>> {
        static_cell::StaticCell::<RefCell<Spi<'static, Blocking>>>::new(RefCell::new(esp)).take()
    }

    pub fn bus(&self) -> &'static RefCell<Spi<'static, Blocking>> {
        self.inner
    }

    #[cfg(feature = "embassy")]
    pub fn into_async(self) -> SpiBus<Async> {
        let esp = self.inner.borrow_mut().into_async();
        SpiBus::<Async>::leak_async(esp)
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
    fn leak_async(
        esp: Spi<'static, Async>,
    ) -> &'static embassy_sync::mutex::Mutex<RefCell<Spi<'static, Async>>> {
        static_cell::StaticCell::<embassy_sync::mutex::Mutex<RefCell<Spi<'static, Async>>>>::new(
            embassy_sync::mutex::Mutex::new(RefCell::new(esp)),
        )
        .take()
    }

    pub fn from_ref(
        bus: &'static embassy_sync::mutex::Mutex<RefCell<Spi<'static, Async>>>,
    ) -> Self {
        SpiBus { inner: bus }
    }

    pub fn into_blocking(self) -> SpiBus<Blocking> {
        let esp = self.inner.into_inner().into_blocking();
        SpiBus::<Blocking>::leak(esp)
    }
}

// ---------------------------------------------------------------------------
// Device (owns CS) — both modes
// ---------------------------------------------------------------------------

pub struct SpiDevice<Blocking> {
    bus: SpiBus<Blocking>,
    cs: Output<'static>,
    delay: Delay,
}

#[cfg(feature = "embassy")]
pub struct SpiDevice<Async> {
    bus: SpiBus<Async>,
    cs: Output<'static>,
    delay: Delay,
}

impl<Dm> SpiDevice<Dm> {
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

impl embedded_hal::spi::ErrorType for SpiDevice<Blocking> {
    type Error = SpiError;
}

/// Run the operations against a `&mut Spi` (works for both modes: esp-hal
/// implements `embedded_hal::spi::SpiBus` for `Spi<'_>` in every `Dm`, §20.5).
/// Generic over `Dm` so one body serves blocking and async.
fn run_ops<Dm: esp_hal::DriverMode>(
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
                Operation::Read(buf) => Spi::read(bus, buf).map_err(SpiError::Bus)?,
                Operation::Write(buf) => Spi::write(bus, buf).map_err(SpiError::Bus)?,
                Operation::Transfer(read, write) => {
                    Spi::transfer(bus, read, write).map_err(SpiError::Bus)?
                }
                Operation::TransferInPlace(buf) => {
                    Spi::transfer_in_place(bus, buf).map_err(SpiError::Bus)?
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

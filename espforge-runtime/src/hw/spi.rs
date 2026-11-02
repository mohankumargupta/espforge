//! `SpiBus` / `SpiDevice`: an SPI master bus shared by its devices.
//!
//! `SpiBus<Dm>` is the shared bus; `SpiDevice<Dm>` is one device that owns its
//! chip-select pin. CS lives in the device, never on the bus. Blocking builds
//! park the bus in a `RefCell`; async builds use an `embassy_sync::Mutex` so a
//! shared `&'static Context` can drive it.
//!
//! `build` is generic over the peripheral instance, so the runtime no longer
//! hardcodes `SPI2`; the generated tokens still spell `SPI2`.

use core::cell::RefCell;
#[cfg(feature = "embassy")]
use esp_hal::Async;
#[cfg(not(feature = "embassy"))]
use esp_hal::Blocking;
use esp_hal::DriverMode;
use esp_hal::gpio::{InputPin, Output, OutputPin};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as EspConfig, Instance, Spi};

use crate::Delay;

/// Minimal SPI configuration: `frequency` + `mode` only.
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
/// esp-hal bus `Error` is private, so `Bus` carries no payload.)
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

#[cfg(not(feature = "embassy"))]
pub struct SpiBus<Dm: DriverMode + 'static> {
    inner: &'static RefCell<Spi<'static, Dm>>,
}

#[cfg(not(feature = "embassy"))]
impl<Dm: DriverMode + 'static> Clone for SpiBus<Dm> {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(not(feature = "embassy"))]
impl<Dm: DriverMode + 'static> Copy for SpiBus<Dm> {}

#[cfg(feature = "embassy")]
pub struct SpiBus<Dm: DriverMode + 'static> {
    inner: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Spi<'static, Dm>,
    >,
}

#[cfg(feature = "embassy")]
impl<Dm: DriverMode + 'static> Clone for SpiBus<Dm> {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(feature = "embassy")]
impl<Dm: DriverMode + 'static> Copy for SpiBus<Dm> {}

// ---------------------------------------------------------------------------
// Blocking build
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl SpiBus<Blocking> {
    pub fn build<I: Instance + 'static>(
        spi: I,
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

    /// Build a bus with no MISO pin: a display or other write-only device.
    pub fn build_write_only<I: Instance + 'static>(
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus<Blocking>, esp_hal::spi::master::ConfigError> {
        static CELL: static_cell::StaticCell<RefCell<Spi<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi);
        let inner = CELL.init(RefCell::new(esp));
        Ok(SpiBus { inner })
    }
}

// ---------------------------------------------------------------------------
// Async build (only under `embassy`)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
impl SpiBus<Async> {
    pub fn build<I: Instance + 'static>(
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus<Async>, esp_hal::spi::master::ConfigError> {
        static CELL: static_cell::StaticCell<
            embassy_sync::mutex::Mutex<
                embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
                Spi<'static, Async>,
            >,
        > = static_cell::StaticCell::new();
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso)
            .into_async();
        let inner = CELL.init(embassy_sync::mutex::Mutex::new(esp));
        Ok(SpiBus { inner })
    }

    pub fn build_write_only<I: Instance + 'static>(
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus<Async>, esp_hal::spi::master::ConfigError> {
        static CELL: static_cell::StaticCell<
            embassy_sync::mutex::Mutex<
                embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
                Spi<'static, Async>,
            >,
        > = static_cell::StaticCell::new();
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .into_async();
        let inner = CELL.init(embassy_sync::mutex::Mutex::new(esp));
        Ok(SpiBus { inner })
    }
}

// ---------------------------------------------------------------------------
// Device (owns CS) — both modes
// ---------------------------------------------------------------------------

type CsCell = RefCell<Output<'static>>;
type DelayCell = RefCell<Delay>;

pub struct SpiDevice<Dm: DriverMode + 'static> {
    bus: SpiBus<Dm>,
    cs: CsCell,
    delay: DelayCell,
}

impl<Dm: DriverMode + 'static> SpiDevice<Dm> {
    /// Attach a device with its own CS pin to an already-built bus.
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

#[cfg(not(feature = "embassy"))]
fn run_ops<Dm: DriverMode + 'static>(
    bus: &mut Spi<'static, Dm>,
    cs: &mut Output<'static>,
    delay: &Delay,
    operations: &mut [embedded_hal::spi::Operation<'_, u8>],
) -> Result<(), SpiError> {
    use embedded_hal::spi::{Operation, SpiBus};
    cs.set_low();
    let result = (|| {
        for op in operations.iter_mut() {
            match op {
                Operation::Read(buf) => bus.read(buf).map_err(|_| SpiError::Bus)?,
                Operation::Write(buf) => bus.write(buf).map_err(|_| SpiError::Bus)?,
                // esp-hal's inherent `transfer` is in-place and shadows the
                // trait, so reach the two-buffer method via UFCS.
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

#[cfg(not(feature = "embassy"))]
impl SpiDevice<Blocking> {
    pub fn transaction(
        &self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        let mut bus = self.bus.inner.borrow_mut();
        let mut cs = self.cs.borrow_mut();
        let delay = self.delay.borrow();
        run_ops(&mut *bus, &mut *cs, &delay, operations)
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Write(bytes)])
    }
    pub fn read(&self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Read(buf)])
    }
    pub fn write_read(&self, w: &[u8], r: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [
            embedded_hal::spi::Operation::Write(w),
            embedded_hal::spi::Operation::Read(r),
        ])
    }
    pub fn transfer_in_place(&self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::TransferInPlace(buf)])
    }
}

#[cfg(feature = "embassy")]
impl SpiDevice<Async> {
    pub async fn transaction(
        &self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        let mut bus = self.bus.inner.lock().await;
        let mut cs = self.cs.borrow_mut();
        let delay = self.delay.borrow();

        cs.set_low();
        let result = async {
            for op in operations.iter_mut() {
                match op {
                    embedded_hal::spi::Operation::Read(buf) => {
                        embedded_hal_async::spi::SpiBus::read(&mut *bus, buf)
                            .await
                            .map_err(|_| SpiError::Bus)?
                    }
                    embedded_hal::spi::Operation::Write(buf) => {
                        embedded_hal_async::spi::SpiBus::write(&mut *bus, buf)
                            .await
                            .map_err(|_| SpiError::Bus)?
                    }
                    embedded_hal::spi::Operation::Transfer(read, write) => {
                        embedded_hal_async::spi::SpiBus::transfer(&mut *bus, read, write)
                            .await
                            .map_err(|_| SpiError::Bus)?
                    }
                    embedded_hal::spi::Operation::TransferInPlace(buf) => {
                        embedded_hal_async::spi::SpiBus::transfer_in_place(&mut *bus, buf)
                            .await
                            .map_err(|_| SpiError::Bus)?
                    }
                    embedded_hal::spi::Operation::DelayNs(ns) => {
                        delay.delay_ns((*ns).try_into().unwrap_or(u32::MAX));
                    }
                }
            }
            Ok(())
        }
        .await;
        cs.set_high();
        result
    }

    pub async fn write(&self, bytes: &[u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Write(bytes)])
            .await
    }
    pub async fn read(&self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Read(buf)])
            .await
    }
    pub async fn write_read(&self, w: &[u8], r: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [
            embedded_hal::spi::Operation::Write(w),
            embedded_hal::spi::Operation::Read(r),
        ])
        .await
    }
    pub async fn transfer_in_place(&self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::TransferInPlace(buf)])
            .await
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

//! `I2cBus`: an I2C master bus shared by reference by its devices.
//!
//! Two flavours, and the scheduler no longer decides which one exists:
//!
//! - [`I2cBus`] is the blocking bus, parked in a `RefCell`. It is compiled in
//!   every build, including an embassy project: a display on an embassy board
//!   still talks over a blocking bus, before the executor starts.
//! - [`AsyncI2cBus`] is the async bus (feature `embassy`), parked in an
//!   `embassy_sync::Mutex`.
//!
//! Device endpoints that ride a blocking bus are `embedded-hal-bus`
//! `RefCellDevice`s, built by the generated wiring: the runtime owns the bus,
//! the standard adapters own the sharing. A device driver receives its address
//! as a constructor argument, so this runtime never rewrites it.
//!
//! `build_in` is generic over the peripheral instance, so `<i2c id="i2c1">`
//! emits `I2C1` and still compiles. The storage lives in an [`I2cBusStorage`]
//! (or [`AsyncI2cBusStorage`]) the *generated* code declares, one `static` per
//! `<i2c>` element, so two buses never share one cell.

use core::cell::RefCell;
use esp_hal::Blocking;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::master::{Config as EspConfig, Error as EspError, I2c, Instance};

/// Minimal I2C configuration: esp-hal's full `Config` carries unstable fields
/// this API deliberately does not expose.
#[derive(Debug, Clone, Copy)]
pub struct I2cConfig {
    pub frequency: esp_hal::time::Rate,
}

impl Default for I2cConfig {
    fn default() -> Self {
        // esp-hal's default is 100kHz.
        I2cConfig {
            frequency: esp_hal::time::Rate::from_khz(100),
        }
    }
}

impl From<I2cConfig> for EspConfig {
    fn from(c: I2cConfig) -> EspConfig {
        EspConfig::default().with_frequency(c.frequency)
    }
}

/// Unified error surfaced to examples. (`ConfigError` does not implement
/// `Eq`, so `I2cError` cannot either.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum I2cError {
    Config(esp_hal::i2c::master::ConfigError),
    Bus(EspError),
}

impl embedded_hal::i2c::Error for I2cError {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        // esp-hal's `Error` does not categorise; surface `Other`.
        embedded_hal::i2c::ErrorKind::Other
    }
}

// ---------------------------------------------------------------------------
// Blocking bus (every build)
// ---------------------------------------------------------------------------

pub struct I2cBus {
    inner: &'static RefCell<I2c<'static, Blocking>>,
}

impl Clone for I2cBus {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for I2cBus {}

/// The `static` one blocking `<i2c>` bus needs.
///
/// The generated `generated.rs` declares one per bus (`__I2C0_STORAGE`), so
/// [`I2cBus::build_in`] can never initialise the same cell twice.
pub struct I2cBusStorage {
    cell: static_cell::StaticCell<RefCell<I2c<'static, Blocking>>>,
}

impl I2cBusStorage {
    pub const fn new() -> Self {
        Self {
            cell: static_cell::StaticCell::new(),
        }
    }
}

impl Default for I2cBusStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl I2cBus {
    /// Build the owned esp-hal `I2c` master into `storage`, and return a `Copy`
    /// `I2cBus` handle. Called once per bus by the generated wiring.
    pub fn build_in<I: Instance + 'static>(
        storage: &'static I2cBusStorage,
        i2c: I,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<I2cBus, esp_hal::i2c::master::ConfigError> {
        let esp = I2c::new(i2c, EspConfig::from(config))?
            .with_sda(sda)
            .with_scl(scl);
        let inner = storage.cell.init(RefCell::new(esp));
        Ok(I2cBus { inner })
    }

    /// The shared cell, for an `embedded-hal-bus` endpoint
    /// (`RefCellDevice::new(bus.cell())`).
    pub fn cell(&self) -> &'static RefCell<I2c<'static, Blocking>> {
        self.inner
    }

    /// Run a sequence of read/write operations against `address`.
    pub fn transaction(
        &self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), I2cError> {
        let mut bus = self.inner.borrow_mut();
        embedded_hal::i2c::I2c::transaction(&mut *bus, address, operations).map_err(I2cError::Bus)
    }

    pub fn write(&self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        let mut bus = self.inner.borrow_mut();
        embedded_hal::i2c::I2c::write(&mut *bus, addr, bytes).map_err(I2cError::Bus)
    }

    pub fn read(&self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        let mut bus = self.inner.borrow_mut();
        embedded_hal::i2c::I2c::read(&mut *bus, addr, buf).map_err(I2cError::Bus)
    }

    pub fn write_read(&self, addr: u8, w: &[u8], r: &mut [u8]) -> Result<(), I2cError> {
        let mut bus = self.inner.borrow_mut();
        embedded_hal::i2c::I2c::write_read(&mut *bus, addr, w, r).map_err(I2cError::Bus)
    }
}

impl embedded_hal::i2c::ErrorType for I2cBus {
    type Error = I2cError;
}

impl embedded_hal::i2c::I2c for I2cBus {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        I2cBus::transaction(self, address, operations)
    }
}

// ---------------------------------------------------------------------------
// Async bus (only under `embassy`)
// ---------------------------------------------------------------------------

/// The `static` one async `<i2c>` bus needs: one mutex-armoured cell per
/// `<i2c>` element.
#[cfg(feature = "embassy")]
pub struct AsyncI2cBusStorage {
    cell: static_cell::StaticCell<
        embassy_sync::mutex::Mutex<
            embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
            I2c<'static, esp_hal::Async>,
        >,
    >,
}

#[cfg(feature = "embassy")]
impl AsyncI2cBusStorage {
    pub const fn new() -> Self {
        Self {
            cell: static_cell::StaticCell::new(),
        }
    }
}

#[cfg(feature = "embassy")]
impl Default for AsyncI2cBusStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "embassy")]
pub struct AsyncI2cBus {
    inner: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        I2c<'static, esp_hal::Async>,
    >,
}

#[cfg(feature = "embassy")]
impl Clone for AsyncI2cBus {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(feature = "embassy")]
impl Copy for AsyncI2cBus {}

#[cfg(feature = "embassy")]
impl AsyncI2cBus {
    pub fn build_in<I: Instance + 'static>(
        storage: &'static AsyncI2cBusStorage,
        i2c: I,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<AsyncI2cBus, esp_hal::i2c::master::ConfigError> {
        let esp = I2c::new(i2c, EspConfig::from(config))?
            .with_sda(sda)
            .with_scl(scl)
            .into_async();
        let inner = storage.cell.init(embassy_sync::mutex::Mutex::new(esp));
        Ok(AsyncI2cBus { inner })
    }

    pub async fn transaction(
        &self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), I2cError> {
        let mut bus = self.inner.lock().await;
        embedded_hal_async::i2c::I2c::transaction(&mut *bus, address, operations)
            .await
            .map_err(I2cError::Bus)
    }

    pub async fn write(&self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        let mut bus = self.inner.lock().await;
        embedded_hal_async::i2c::I2c::write(&mut *bus, addr, bytes)
            .await
            .map_err(I2cError::Bus)
    }

    pub async fn read(&self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        let mut bus = self.inner.lock().await;
        embedded_hal_async::i2c::I2c::read(&mut *bus, addr, buf)
            .await
            .map_err(I2cError::Bus)
    }

    pub async fn write_read(&self, addr: u8, w: &[u8], r: &mut [u8]) -> Result<(), I2cError> {
        let mut bus = self.inner.lock().await;
        embedded_hal_async::i2c::I2c::write_read(&mut *bus, addr, w, r)
            .await
            .map_err(I2cError::Bus)
    }
}

#[cfg(feature = "embassy")]
impl embedded_hal::i2c::ErrorType for AsyncI2cBus {
    type Error = I2cError;
}

#[cfg(feature = "embassy")]
impl embedded_hal_async::i2c::I2c for AsyncI2cBus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        AsyncI2cBus::transaction(self, address, operations).await
    }
}

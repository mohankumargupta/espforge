//! `I2cBus`: an I2C master bus shared by reference by its components.
//!
//! One struct parametric over the esp-hal mode typestate `Dm`: the blocking
//! build parks the bus in a `RefCell`, the async build in an
//! `embassy_sync::Mutex`, so a shared `&'static Context` can drive it without
//! a `&mut` borrow.
//!
//! `build` is generic over the peripheral instance, so `<i2c id="i2c1">`
//! emits `I2C1` and still compiles. The tokens in the goldens stay `I2C0`.

#[cfg(not(feature = "embassy"))]
use core::cell::RefCell;
#[cfg(feature = "embassy")]
use esp_hal::Async;
#[cfg(not(feature = "embassy"))]
use esp_hal::Blocking;
use esp_hal::DriverMode;
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

#[cfg(not(feature = "embassy"))]
pub struct I2cBus<Dm: DriverMode + 'static> {
    inner: &'static RefCell<I2c<'static, Dm>>,
}

#[cfg(not(feature = "embassy"))]
impl<Dm: DriverMode + 'static> Clone for I2cBus<Dm> {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(not(feature = "embassy"))]
impl<Dm: DriverMode + 'static> Copy for I2cBus<Dm> {}

#[cfg(feature = "embassy")]
pub struct I2cBus<Dm: DriverMode + 'static> {
    inner: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        I2c<'static, Dm>,
    >,
}

#[cfg(feature = "embassy")]
impl<Dm: DriverMode + 'static> Clone for I2cBus<Dm> {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(feature = "embassy")]
impl<Dm: DriverMode + 'static> Copy for I2cBus<Dm> {}

// ---------------------------------------------------------------------------
// Blocking build + inherent transaction + convenience API
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl I2cBus<Blocking> {
    /// Build the owned esp-hal `I2c` master, park it in a `static` cell, and
    /// return a `Copy` `I2cBus` handle. Called once by the generated wiring.
    /// Fallible: the generated code `.expect`s the `ConfigError`.
    pub fn build<I: Instance + 'static>(
        i2c: I,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<I2cBus<Blocking>, esp_hal::i2c::master::ConfigError> {
        static CELL: static_cell::StaticCell<RefCell<I2c<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = I2c::new(i2c, EspConfig::from(config))?
            .with_sda(sda)
            .with_scl(scl);
        let inner = CELL.init(RefCell::new(esp));
        Ok(I2cBus { inner })
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

#[cfg(not(feature = "embassy"))]
impl embedded_hal::i2c::ErrorType for I2cBus<Blocking> {
    type Error = I2cError;
}

#[cfg(not(feature = "embassy"))]
impl embedded_hal::i2c::I2c for I2cBus<Blocking> {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        I2cBus::transaction(self, address, operations)
    }
}

// ---------------------------------------------------------------------------
// Async build + inherent transaction (only under `embassy`)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
impl I2cBus<Async> {
    pub fn build<I: Instance + 'static>(
        i2c: I,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<I2cBus<Async>, esp_hal::i2c::master::ConfigError> {
        static CELL: static_cell::StaticCell<
            embassy_sync::mutex::Mutex<
                embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
                I2c<'static, Async>,
            >,
        > = static_cell::StaticCell::new();
        let esp = I2c::new(i2c, EspConfig::from(config))?
            .with_sda(sda)
            .with_scl(scl)
            .into_async();
        let inner = CELL.init(embassy_sync::mutex::Mutex::new(esp));
        Ok(I2cBus { inner })
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
impl embedded_hal::i2c::ErrorType for I2cBus<Async> {
    type Error = I2cError;
}

#[cfg(feature = "embassy")]
impl embedded_hal_async::i2c::I2c for I2cBus<Async> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        I2cBus::transaction(self, address, operations).await
    }
}

// ---------------------------------------------------------------------------
// An addressed endpoint on a shared bus
// ---------------------------------------------------------------------------

/// An addressed endpoint on a shared I2C bus.
///
/// The address comes from `<i2c_component address="..">`, so the project file
/// stays the single source of truth: neither `app.rs` nor a driver re-types it.
/// Reads and writes take no address because this handle already knows its
/// target, and several `I2cDevice`s on one controller are the normal
/// multi-drop case.
///
/// When this handle is used through the `embedded-hal` `I2c` traits (what a
/// third-party driver such as the SSD1306 crate does), the address the driver
/// passes is **ignored** and [`address`](Self::address) is used instead. One
/// handle therefore drives exactly one part.
pub struct I2cDevice<Dm: DriverMode + 'static> {
    bus: I2cBus<Dm>,
    address: u8,
}

impl<Dm: DriverMode + 'static> Clone for I2cDevice<Dm> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Dm: DriverMode + 'static> Copy for I2cDevice<Dm> {}

impl<Dm: DriverMode + 'static> I2cDevice<Dm> {
    /// Wrap `bus`, targeting the 7-bit slave `address`.
    pub fn new(bus: I2cBus<Dm>, address: u8) -> Self {
        Self { bus, address }
    }

    /// The 7-bit address this endpoint talks to, as configured in `.espf`.
    pub fn address(&self) -> u8 {
        self.address
    }

    /// The raw, unaddressed bus this device rides on.
    pub fn bus(&self) -> I2cBus<Dm> {
        self.bus
    }
}

#[cfg(not(feature = "embassy"))]
impl I2cDevice<Blocking> {
    /// Write `bytes` to this device's address.
    pub fn write(&self, bytes: &[u8]) -> Result<(), I2cError> {
        self.bus.write(self.address, bytes)
    }

    /// Read into `buf` from this device's address.
    pub fn read(&self, buf: &mut [u8]) -> Result<(), I2cError> {
        self.bus.read(self.address, buf)
    }

    /// Write `write`, then read into `read`, in one transaction.
    pub fn write_read(&self, write: &[u8], read: &mut [u8]) -> Result<(), I2cError> {
        self.bus.write_read(self.address, write, read)
    }

    /// Run a sequence of operations against this device's address.
    pub fn transaction(
        &self,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), I2cError> {
        self.bus.transaction(self.address, operations)
    }
}

#[cfg(not(feature = "embassy"))]
impl embedded_hal::i2c::ErrorType for I2cDevice<Blocking> {
    type Error = I2cError;
}

/// The address a driver passes is ignored: this handle talks to the address
/// configured in `.espf`. The trait's provided `read`/`write`/`write_read`
/// route through `transaction`, so they inherit that contract.
#[cfg(not(feature = "embassy"))]
impl embedded_hal::i2c::I2c for I2cDevice<Blocking> {
    fn transaction(
        &mut self,
        _address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        I2cDevice::transaction(self, operations)
    }
}

#[cfg(feature = "embassy")]
impl I2cDevice<Async> {
    /// Write `bytes` to this device's address.
    pub async fn write(&self, bytes: &[u8]) -> Result<(), I2cError> {
        self.bus.write(self.address, bytes).await
    }

    /// Read into `buf` from this device's address.
    pub async fn read(&self, buf: &mut [u8]) -> Result<(), I2cError> {
        self.bus.read(self.address, buf).await
    }

    /// Write `write`, then read into `read`, in one transaction.
    pub async fn write_read(&self, write: &[u8], read: &mut [u8]) -> Result<(), I2cError> {
        self.bus.write_read(self.address, write, read).await
    }

    /// Run a sequence of operations against this device's address.
    pub async fn transaction(
        &self,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), I2cError> {
        self.bus.transaction(self.address, operations).await
    }
}

#[cfg(feature = "embassy")]
impl embedded_hal::i2c::ErrorType for I2cDevice<Async> {
    type Error = I2cError;
}

/// The address a driver passes is ignored: this handle talks to the address
/// configured in `.espf`.
#[cfg(feature = "embassy")]
impl embedded_hal_async::i2c::I2c for I2cDevice<Async> {
    async fn transaction(
        &mut self,
        _address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        I2cDevice::transaction(self, operations).await
    }
}

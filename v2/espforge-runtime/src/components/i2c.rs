//! `i2c` component: an I2C master bus, shared by reference by many devices
//! (ADR-003/008 bus-sharing).
//!
//! Parametric over the esp-hal mode typestate `Dm` (design §20.2): `I2cBus<Blocking>`
//! and `I2cBus<Async>` are distinct types, exactly mirroring `esp_hal::i2c::master::I2c`.
//! The generated project pins `Dm` from the YAML `runtime:` field; this crate stays
//! mode-blind and generic.
//!
//! Interior mutability is keyed on `Dm` (§20.3): the blocking variant uses a plain
//! `RefCell` (no yield, safe to hold the borrow); the async variant uses an
//! `embassy_sync::Mutex<RefCell<..>>` so the bus can be shared across Embassy tasks
//! safely (the lock is released while waiting, never held across the `.await`).
//!
//! The blocking variant also implements `embedded_hal::i2c::I2c` so it can be passed
//! directly to drivers (e.g. `ssd1306`) that expect the trait object.

use core::cell::RefCell;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::master::{Config as EspConfig, Error as EspError, I2c};
use esp_hal::{Blocking, DriverMode};
#[cfg(feature = "embassy")]
use esp_hal::Async;
use esp_hal::peripherals::I2C0;

/// Minimal YAML-facing I2C config (design §20.6, Level B). esp-hal's full
/// `Config` carries unstable/cfg-gated timeout + FSM fields we deliberately do
/// not expose; the codegen converts this into `esp_hal::i2c::master::Config`.
#[derive(Debug, Clone, Copy)]
pub struct I2cConfig {
    pub frequency: esp_hal::time::Rate,
}

impl Default for I2cConfig {
    fn default() -> Self {
        // esp-hal default is 100kHz.
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

/// Unified error surfaced to examples. `Config` carries the `ConfigError` from
/// `build()`; `Bus` carries the runtime transaction error. (`ConfigError` does
/// not implement `Eq`, so `I2cError` cannot either.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum I2cError {
    Config(esp_hal::i2c::master::ConfigError),
    Bus(EspError),
}

impl embedded_hal::i2c::Error for I2cError {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match self {
            I2cError::Config(_) => embedded_hal::i2c::ErrorKind::Other,
            I2cError::Bus(_) => embedded_hal::i2c::ErrorKind::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Blocking variant — owns the `RefCell<I2c>` by value (move-by-value, §12).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct I2cBus<Dm: DriverMode + 'static> {
    inner: &'static RefCell<I2c<'static, Dm>>,
}

impl I2cBus<Blocking> {
    /// Build the owned esp-hal `I2c` master and park it in a `static` cell,
    /// returning a `Copy` `I2cBus` handle. Called once by the generated wiring;
    /// the `&'static` is safe because the cell is a `static` item. **Fallible**
    /// (design §20.7): the codegen `.expect`s the `ConfigError` in the generated
    /// `setup` with a component-specific message.
    ///
    /// esp-hal 1.1: `new(config)` only — pins attached via `with_sda`/`with_scl`;
    /// there is **no `clocks` argument** (§20.1).
    pub fn build(
        i2c: I2C0<'static>,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<I2cBus<Blocking>, esp_hal::i2c::master::ConfigError> {
        static CELL: static_cell::StaticCell<RefCell<I2c<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = I2c::new(i2c, EspConfig::from(config))?.with_sda(sda).with_scl(scl);
        let inner = CELL.init(RefCell::new(esp));
        Ok(I2cBus { inner })
    }

    /// Morph this bus into its async counterpart (design §20.1). The underlying
    /// `I2c` is moved out of the `RefCell`, morphed, and re-parked in a `static`
    /// async cell.
    #[cfg(feature = "embassy")]
    pub fn into_async(self) -> I2cBus<Async> {
        static CELL: static_cell::StaticCell<RefCell<I2c<'static, Async>>> =
            static_cell::StaticCell::new();
        let esp = self.inner.borrow_mut().into_async();
        let inner = CELL.init(RefCell::new(esp));
        I2cBus { inner }
    }
}

impl embedded_hal::i2c::ErrorType for I2cBus<Blocking> {
    type Error = I2cError;
}

impl embedded_hal::i2c::I2c for I2cBus<Blocking> {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        let mut bus = self.inner.borrow_mut();
        for op in operations.iter_mut() {
            match op {
                embedded_hal::i2c::Operation::Write(buffer) => {
                    bus.write(address, buffer).map_err(I2cError::Bus)?
                }
                embedded_hal::i2c::Operation::Read(buffer) => {
                    bus.read(address, buffer).map_err(I2cError::Bus)?
                }
            }
        }
        Ok(())
    }
}

// Convenience blocking helpers (idiomatic, transaction-style per §20.5).
impl I2cBus<Blocking> {
    pub fn write(&self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        self.inner.borrow_mut().write(addr, bytes).map_err(I2cError::Bus)
    }
    pub fn read(&self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        self.inner.borrow_mut().read(addr, buf).map_err(I2cError::Bus)
    }
    pub fn write_read(&self, addr: u8, w: &[u8], r: &mut [u8]) -> Result<(), I2cError> {
        self.inner
            .borrow_mut()
            .write_read(addr, w, r)
            .map_err(I2cError::Bus)
    }
}

// ---------------------------------------------------------------------------
// Async variant (only compiled under `embassy`, §20.2/§20.3)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
pub struct I2cBus<Async> {
    inner: &'static embassy_sync::mutex::Mutex<RefCell<I2c<'static, Async>>>,
}

#[cfg(feature = "embassy")]
impl I2cBus<Async> {
    /// Morph an async bus back to blocking (design §20.1).
    pub fn into_blocking(self) -> I2cBus<Blocking> {
        static CELL: static_cell::StaticCell<RefCell<I2c<'static, Blocking>>> =
            static_cell::StaticCell::new();
        let esp = self.inner.into_inner().into_blocking();
        let inner = CELL.init(RefCell::new(esp));
        I2cBus { inner }
    }

    /// Lock the bus and run an async transaction atomically across tasks (§20.4).
    pub async fn transaction(
        &self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), I2cError> {
        let mut guard = self.inner.lock().await;
        guard
            .transaction_async(address, operations)
            .await
            .map_err(I2cError::Bus)
    }

    pub async fn write(&self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        let mut guard = self.inner.lock().await;
        guard.write_async(addr, bytes).await.map_err(I2cError::Bus)
    }
    pub async fn read(&self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        let mut guard = self.inner.lock().await;
        guard.read_async(addr, buf).await.map_err(I2cError::Bus)
    }
    pub async fn write_read(&self, addr: u8, w: &[u8], r: &mut [u8]) -> Result<(), I2cError> {
        let mut guard = self.inner.lock().await;
        guard
            .write_read_async(addr, w, r)
            .await
            .map_err(I2cError::Bus)
    }
}

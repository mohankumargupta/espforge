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
#[cfg(not(feature = "embassy"))]
use esp_hal::Blocking;
use esp_hal::DriverMode;
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
        // esp-hal's `Error` does not categorise; surface `Other`.
        embedded_hal::i2c::ErrorKind::Other
    }
}

// ---------------------------------------------------------------------------
// One parametric struct; the inner sharing primitive differs by build.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
pub struct I2cBus<Dm: DriverMode + 'static> {
    inner: &'static RefCell<I2c<'static, Dm>>,
}

// Manually impl Clone+Copy so Dm doesn't need to be Copy.
// The struct only holds a &'static reference, which is always Copy.
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
        RefCell<I2c<'static, Dm>>,
    >,
}

// Manually impl Clone+Copy so Dm doesn't need to be Copy.
// The struct only holds a &'static reference, which is always Copy.
#[cfg(feature = "embassy")]
impl<Dm: DriverMode + 'static> Clone for I2cBus<Dm> {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(feature = "embassy")]
impl<Dm: DriverMode + 'static> Copy for I2cBus<Dm> {}

// ---------------------------------------------------------------------------
// Blocking build + API
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl I2cBus<Blocking> {
    /// Build the owned esp-hal `I2c` master, park it in a `static` cell, and
    /// return a `Copy` `I2cBus` handle. Called once by the generated wiring.
    /// **Fallible** (design §20.7): the codegen `.expect`s the `ConfigError`
    /// with a component-specific message.
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
#[cfg(not(feature = "embassy"))]
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
// Async build + API (only compiled under `embassy`, §20.2/§20.3)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
impl I2cBus<Async> {
    /// Build the owned esp-hal `I2c` master (async mode) and park it in a
    /// mutex-backed `static` cell. **Fallible** per §20.7.
    pub fn build(
        i2c: I2C0<'static>,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<I2cBus<Async>, esp_hal::i2c::master::ConfigError> {
        static CELL: static_cell::StaticCell<
            embassy_sync::mutex::Mutex<
                embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
                RefCell<I2c<'static, Async>>,
            >,
        > = static_cell::StaticCell::new();
        let esp = I2c::new(i2c, EspConfig::from(config))?
            .with_sda(sda)
            .with_scl(scl)
            .into_async();
        let inner = CELL.init(embassy_sync::mutex::Mutex::new(RefCell::new(esp)));
        Ok(I2cBus { inner })
    }

    /// Lock the bus and run an async transaction atomically across tasks (§20.4).
    /// Iterates the `embedded_hal::i2c::Operation` slice and dispatches to
    /// esp-hal's async `write_async`/`read_async` per op, holding the lock for
    /// the whole sequence (so the transaction is atomic across tasks). This
    /// avoids esp-hal's distinct `Operation` enum and any heap allocation.
    pub async fn transaction(
        &self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), I2cError> {
        let guard = self.inner.lock().await;
        let mut bus = guard.borrow_mut();
        for op in operations.iter_mut() {
            match op {
                embedded_hal::i2c::Operation::Write(buffer) => {
                    bus.write_async(address, buffer).await.map_err(I2cError::Bus)?
                }
                embedded_hal::i2c::Operation::Read(buffer) => {
                    bus.read_async(address, buffer).await.map_err(I2cError::Bus)?
                }
            }
        }
        Ok(())
    }

    pub async fn write(&self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        let guard = self.inner.lock().await;
        guard.borrow_mut().write_async(addr, bytes).await.map_err(I2cError::Bus)
    }
    pub async fn read(&self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        let guard = self.inner.lock().await;
        guard.borrow_mut().read_async(addr, buf).await.map_err(I2cError::Bus)
    }
    pub async fn write_read(&self, addr: u8, w: &[u8], r: &mut [u8]) -> Result<(), I2cError> {
        let guard = self.inner.lock().await;
        guard
            .borrow_mut()
            .write_read_async(addr, w, r)
            .await
            .map_err(I2cError::Bus)
    }
}

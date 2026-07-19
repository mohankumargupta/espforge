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
use esp_hal::mode::{Async, Blocking};
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
            frequency: EspConfig::default().frequency,
        }
    }
}

impl From<I2cConfig> for EspConfig {
    fn from(c: I2cConfig) -> EspConfig {
        EspConfig {
            frequency: c.frequency,
            ..EspConfig::default()
        }
    }
}

/// Unified error surfaced to examples. `Config` carries the `ConfigError` from
/// `build()`; `Bus` carries the runtime transaction error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cError {
    Config(esp_hal::i2c::master::ConfigError),
    Bus(EspError),
}

// ---------------------------------------------------------------------------
// Blocking variant
// ---------------------------------------------------------------------------

/// Shared I2C master bus (blocking). `Copy`-cheap via interior `RefCell`.
#[derive(Clone, Copy)]
pub struct I2cBus<Blocking> {
    inner: &'static RefCell<I2c<'static, Blocking>>,
}

impl I2cBus<Blocking> {
    /// Wrap a `&'static RefCell<I2c>` allocated by the generated wiring. The
    /// `i2c` driver builds the inner `I2c` once and hands out `Copy` handles to
    /// every device that shares this bus.
    pub fn from_ref(bus: &'static RefCell<I2c<'static, Blocking>>) -> Self {
        I2cBus { inner: bus }
    }

    /// Build the owned esp-hal `I2c` master. Called once by the generated
    /// wiring; the result is parked in a `StaticCell<RefCell<_>>` and surfaced
    /// via `from_ref`. **Fallible** (design §20.7): the codegen `.expect`s the
    /// `ConfigError` in the generated `setup` with a component-specific message.
    ///
    /// esp-hal 1.1: `new(config)` only — pins attached via `with_sda`/`with_scl`;
    /// there is **no `clocks` argument** (§20.1).
    pub fn build(
        i2c: I2C0<'static>,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
        config: I2cConfig,
    ) -> Result<&'static RefCell<I2c<'static, Blocking>>, esp_hal::i2c::master::ConfigError> {
        let esp = I2c::new(i2c, EspConfig::from(config))?.with_sda(sda).with_scl(scl);
        Ok(I2cBus::<Blocking>::leak(esp))
    }

    /// Park an owned `I2c` in a `StaticCell` and return the `&'static RefCell`
    /// so `from_ref` can hand out `Copy` handles.
    fn leak(esp: I2c<'static, Blocking>) -> &'static RefCell<I2c<'static, Blocking>> {
        static_cell::StaticCell::<RefCell<I2c<'static, Blocking>>>::new(RefCell::new(esp))
            .take()
    }

    /// Shared access to the underlying `RefCell<I2c>` (advanced use).
    pub fn bus(&self) -> &'static RefCell<I2c<'static, Blocking>> {
        self.inner
    }

    /// Morph this bus into its async counterpart (design §20.1). The underlying
    /// `I2c` is moved out of the `RefCell`, morphed, and re-parked.
    #[cfg(feature = "embassy")]
    pub fn into_async(self) -> I2cBus<Async> {
        let esp = self.inner.borrow_mut().into_async();
        I2cBus::<Async>::leak(esp)
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
    fn leak_async(esp: I2c<'static, Async>) -> &'static embassy_sync::mutex::Mutex<RefCell<I2c<'static, Async>>> {
        static_cell::StaticCell::<
            embassy_sync::mutex::Mutex<RefCell<I2c<'static, Async>>>,
        >::new(embassy_sync::mutex::Mutex::new(RefCell::new(esp)))
        .take()
    }

    pub fn from_ref(
        bus: &'static embassy_sync::mutex::Mutex<RefCell<I2c<'static, Async>>>,
    ) -> Self {
        I2cBus { inner: bus }
    }

    /// Morph an async bus back to blocking (design §20.1).
    pub fn into_blocking(self) -> I2cBus<Blocking> {
        let esp = self.inner.into_inner().into_blocking();
        I2cBus::<Blocking>::leak(esp)
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

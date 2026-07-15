//! `i2c` component: an I2C master bus, shared by reference by many devices
//! (ADR-003/008 bus-sharing).
//!
//! Mirrors v1's `espforge_components::components::i2c::I2C`: a `Copy` handle
//! around a `&'static RefCell<I2c>`. The actual peripheral is allocated once in
//! a `StaticCell` by the generated wiring (the `i2c` driver's `construct`),
//! so this type is cheap to move into devices — copying the handle is a
//! pointer bitcopy, never a move of the peripheral (no double-move, ADR-008).

use core::cell::RefCell;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::master::{Config, I2c};

/// A `Copy` handle to a shared I2C master bus (v1-style, ADR-003).
#[derive(Clone, Copy)]
pub struct I2cBus {
    bus: &'static RefCell<I2c<'static, esp_hal::Blocking>>,
}

impl I2cBus {
    /// Wrap a `&'static RefCell<I2c>` allocated by the wiring code. The `i2c`
    /// component driver builds the inner `I2c` once and hands out `Copy`
    /// handles to every device that shares this bus.
    pub fn from_ref(bus: &'static RefCell<I2c<'static, esp_hal::Blocking>>) -> Self {
        I2cBus { bus }
    }

    /// Build the owned esp-hal `I2c` master from its peripheral + sda/scl pins.
    /// Called once by the generated wiring; the result is parked in a
    /// `StaticCell<RefCell<_>>` and surfaced via `from_ref`.
    pub fn build(
        i2c: esp_hal::peripherals::I2C0<'static>,
        sda: impl OutputPin + 'static + InputPin,
        scl: impl OutputPin + 'static + InputPin,
    ) -> I2c<'static, esp_hal::Blocking> {
        I2c::new(i2c, Config::default())
            .unwrap()
            .with_sda(sda)
            .with_scl(scl)
    }

    /// Shared access to the underlying bus.
    pub fn bus(&self) -> &'static RefCell<I2c<'static, esp_hal::Blocking>> {
        self.bus
    }
}

// Let `I2cBus` itself be moved by value into display-interface style
// consumers (e.g. `ssd1306::I2CDisplayInterface`), mirroring v1's
// `espforge_components::components::i2c::I2C` impl.
impl embedded_hal::i2c::ErrorType for I2cBus {
    type Error = esp_hal::i2c::master::Error;
}

impl embedded_hal::i2c::I2c for I2cBus {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.bus.borrow_mut().transaction(address, operations)
    }
}

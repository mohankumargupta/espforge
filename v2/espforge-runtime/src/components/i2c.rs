//! `i2c` component: an I2C master bus, shared by reference by many devices
//! (ADR-003/008 bus-sharing).

use esp_hal::i2c::master::I2c;

pub struct I2cBus {
    bus: I2c<'static, esp_hal::peripherals::I2C0>,
}

impl I2cBus {
    /// `bus` is the I2C0 peripheral moved in by value. Devices borrow `&I2cBus`
    /// (shared access) to talk on the same bus.
    pub fn new(bus: I2c<'static, esp_hal::peripherals::I2C0>) -> Self {
        I2cBus { bus }
    }

    /// Shared access to the underlying bus.
    pub fn bus(&self) -> &I2c<'static, esp_hal::peripherals::I2C0> {
        &self.bus
    }

    /// Mutable access to the underlying bus.
    pub fn bus_mut(&mut self) -> &mut I2c<'static, esp_hal::peripherals::I2C0> {
        &mut self.bus
    }
}

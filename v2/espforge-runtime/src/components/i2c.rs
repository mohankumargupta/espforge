//! `i2c` component: an I2C master bus, shared by reference by many devices
//! (ADR-003/008 bus-sharing).

use esp_hal::gpio::OutputPin;
use esp_hal::i2c::master::{Config, I2c};

pub struct I2cBus {
    bus: I2c<'static, esp_hal::Blocking>,
}

impl I2cBus {
    /// `i2c` is the I2C peripheral moved in by value; `sda`/`scl` are the bus
    /// pins moved in by value. Devices borrow `&I2cBus` (shared access) to talk
    /// on the same bus.
    pub fn new(
        i2c: esp_hal::peripherals::I2C0<'static>,
        sda: impl OutputPin + 'static + esp_hal::gpio::InputPin,
        scl: impl OutputPin + 'static + esp_hal::gpio::InputPin ,
    ) -> Self {
        let bus = I2c::new(i2c, Config::default())
            .unwrap()
            .with_sda(sda)
            .with_scl(scl);
        I2cBus { bus }
    }

    /// Shared access to the underlying bus.
    pub fn bus(&self) -> &I2c<'static, esp_hal::Blocking> {
        &self.bus
    }

    /// Mutable access to the underlying bus.
    pub fn bus_mut(&mut self) -> &mut I2c<'static, esp_hal::Blocking> {
        &mut self.bus
    }
}

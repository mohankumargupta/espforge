use core::cell::RefCell;
use embedded_hal::i2c::{ErrorType, I2c, Operation};
use espforge_platform::esp_hal::Blocking;
use espforge_platform::esp_hal::i2c::master::I2c as HalI2c;

pub use espforge_common::components::i2c::I2cDeviceConfig;

#[derive(Copy, Clone)]
pub struct I2C {
    pub device: espforge_platform::bus::I2cDevice<'static>,
}

impl I2C {
    pub fn new(bus: &'static RefCell<HalI2c<'static, Blocking>>) -> Self {
        Self {
            device: espforge_platform::bus::I2cDevice::new(bus),
        }
    }
}

impl ErrorType for I2C {
    type Error = <espforge_platform::bus::I2cDevice<'static> as ErrorType>::Error;
}

impl I2c for I2C {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.device.transaction(address, operations)
    }
}

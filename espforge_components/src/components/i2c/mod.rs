use core::cell::RefCell;
use espforge_platform::esp_hal::i2c::master::I2c as HalI2c;
use espforge_platform::esp_hal::Blocking;
use embedded_hal::i2c::{I2c, Operation, ErrorType};

pub use espforge_common::components::i2c::I2cDeviceConfig;

#[derive(Copy, Clone)]
pub struct I2C<'a> {
    pub device: espforge_platform::bus::I2cDevice<'a>,
}

impl<'a> I2C<'a> {
    pub fn new(bus: &'a RefCell<HalI2c<'static, Blocking>>) -> Self {
        Self {
            device: espforge_platform::bus::I2cDevice::new(bus),
        }
    }
}

impl<'a> ErrorType for I2C<'a> {
    type Error = <espforge_platform::bus::I2cDevice<'a> as ErrorType>::Error;
}

impl<'a> I2c for I2C<'a> {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        self.device.transaction(address, operations)
    }
}
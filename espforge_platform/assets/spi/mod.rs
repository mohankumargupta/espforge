use core::cell::RefCell;
use esp_hal::{
    spi::master::Spi,
    i2c::master::I2c,
    gpio::{AnyPin, Output, OutputConfig, Level},
    Blocking,
    delay::Delay,
};
use embedded_hal_bus::{
    spi::RefCellDevice as SpiRefCellDevice, 
    i2c::RefCellDevice as I2cRefCellDevice
};
use embedded_hal::i2c::{I2c as I2cTrait, ErrorType as I2cErrorType, Operation as I2cOperation};
use embedded_hal::spi::{SpiDevice as SpiDeviceTrait, ErrorType as SpiErrorType, Operation as SpiOperation};

// ... (Keep SpiDevice struct from previous step) ...

// --- I2C Device Implementation ---

pub struct I2cDevice<'a> {
    // RefCellDevice handles the borrowing logic for us
    inner: I2cRefCellDevice<'a, I2c<'static, Blocking>>,
}

impl<'a> I2cDevice<'a> {
    pub fn new(bus: &'a RefCell<I2c<'static, Blocking>>) -> Self {
        // embedded-hal-bus creates a device that borrows the bus only during transactions
        let dev = I2cRefCellDevice::new(bus);
        Self { inner: dev }
    }
}

// 1. Implement ErrorType
impl<'a> I2cErrorType for I2cDevice<'a> {
    type Error = esp_hal::i2c::master::Error;
}

// 2. Implement I2c Trait
impl<'a> I2cTrait<u8> for I2cDevice<'a> {
    fn transaction(&mut self, address: u8, operations: &mut [I2cOperation<'_>]) -> Result<(), Self::Error> {
        self.inner.transaction(address, operations)
    }
}

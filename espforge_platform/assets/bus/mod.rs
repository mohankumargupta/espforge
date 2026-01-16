use core::cell::RefCell;
use esp_hal::{
    spi::master::Spi,
    i2c::master::I2c,
    gpio::{Output},
    Blocking,
    delay::Delay,
};
use embedded_hal_bus::{spi::RefCellDevice as SpiRefCellDevice, i2c::RefCellDevice as I2cRefCellDevice};

pub struct SpiDevice<'a> {
    inner: SpiRefCellDevice<'a, Spi<'static, Blocking>, Output<'static>, Delay>,
}


impl<'a> embedded_hal::spi::SpiDevice for SpiDevice<'a> {
    fn transaction(&mut self, operations: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.inner.transaction(operations)
    }
}

impl<'a> embedded_hal::spi::ErrorType for SpiDevice<'a> {
    type Error = <SpiRefCellDevice<'a, Spi<'static, Blocking>, Output<'static>, Delay> as embedded_hal::spi::ErrorType>::Error;
}

impl<'a> embedded_hal::i2c::I2c for I2cDevice<'a> {
    fn transaction(&mut self, address: u8, operations: &mut [embedded_hal::i2c::Operation<'_>]) -> Result<(), Self::Error> {
        self.inner.transaction(address, operations)
    }
}

impl<'a> embedded_hal::i2c::ErrorType for I2cDevice<'a> {
    type Error = <I2cRefCellDevice<'a, I2c<'static, Blocking>> as embedded_hal::i2c::ErrorType>::Error;
}

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


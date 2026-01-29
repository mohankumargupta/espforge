use core::cell::RefCell;
use esp_hal::spi::master::Spi;
use esp_hal::Blocking;
use crate::gpio::GPIOOutput;

pub struct SpiDevice<'a> {
    bus: &'a RefCell<Spi<'static, Blocking>>,
    cs: GPIOOutput,
}

impl<'a> SpiDevice<'a> {
    pub fn new(bus: &'a RefCell<Spi<'static, Blocking>>, cs: GPIOOutput) -> Self {
        Self { bus, cs }
    }

    pub fn bus(&self) -> &'a RefCell<Spi<'static, Blocking>> {
        self.bus
    }
}

impl<'a> embedded_hal::spi::ErrorType for SpiDevice<'a> {
    type Error = esp_hal::spi::Error;
}

impl<'a> embedded_hal::spi::SpiDevice for SpiDevice<'a> {
    fn transaction(&mut self, operations: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), Self::Error> {
        let mut bus = self.bus.borrow_mut();
        // Create a temporary ExclusiveDevice to manage CS and transaction
        // We use esp_hal::delay::Delay directly as it implements DelayNs
        let delay = esp_hal::delay::Delay::new();
        // ExclusiveDevice::new returns Result in embedded-hal-bus 0.3.0, so we unwrap (it fails only on pin misuse usually)
        let mut dev = embedded_hal_bus::spi::ExclusiveDevice::new(&mut *bus, &mut self.cs, delay).unwrap();
           dev.transaction(operations).map_err(|e| match e {
            embedded_hal_bus::spi::DeviceError::Spi(e) => e,
            embedded_hal_bus::spi::DeviceError::Cs(_) => unreachable!("CS pin error should be impossible"),
        })
    }
}

#[derive(Copy, Clone)]
pub struct I2cDevice<'a> {
    bus: &'a RefCell<esp_hal::i2c::master::I2c<'static, Blocking>>,
}

impl<'a> I2cDevice<'a> {
    pub fn new(bus: &'a RefCell<esp_hal::i2c::master::I2c<'static, Blocking>>) -> Self {
        Self { bus }
    }

    pub fn bus(&self) -> &'a RefCell<esp_hal::i2c::master::I2c<'static, Blocking>> {
        self.bus
    }
}

impl<'a> embedded_hal::i2c::ErrorType for I2cDevice<'a> {
    type Error = esp_hal::i2c::master::Error;
}

impl<'a> embedded_hal::i2c::I2c for I2cDevice<'a> {
    fn transaction(&mut self, address: u8, operations: &mut [embedded_hal::i2c::Operation<'_>]) -> Result<(), Self::Error> {
        // Create a temporary RefCellDevice to manage shared bus access
        let mut dev = embedded_hal_bus::i2c::RefCellDevice::new(self.bus);
        dev.transaction(address, operations)
    }
}

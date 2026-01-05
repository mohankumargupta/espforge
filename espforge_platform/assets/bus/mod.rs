use core::cell::RefCell;
use esp_hal::{
    spi::master::Spi,
    i2c::master::I2c,
    gpio::AnyPin,
    Blocking,
    delay::Delay,
};
use embedded_hal_bus::{spi::RefCellDevice as SpiRefCellDevice, i2c::RefCellDevice as I2cRefCellDevice};

/// Wrapper for a device on a shared SPI bus.
///
/// This wrapper handles the Chip Select (CS) pin and exclusive access
/// to the shared SPI bus during transactions.
pub struct SpiDevice<'a> {
    inner: SpiRefCellDevice<'a, Spi<'static, Blocking>, AnyPin<'static>, Delay>,
}

impl<'a> SpiDevice<'a> {
    /// Create a new device sharing the SPI bus
    ///
    /// # Arguments
    /// * `bus` - Reference to the shared SPI bus
    /// * `cs` - The Chip Select pin for this specific device
    pub fn new(bus: &'a RefCell<Spi<'static, Blocking>>, cs: AnyPin<'static>) -> Self {
        let delay = Delay::new();
        // We expect the bus to be valid. In embedded contexts, panic on init failure is acceptable.
        let dev = SpiRefCellDevice::new(bus, cs, delay).expect("Failed to create SPI device");
        Self { inner: dev }
    }
}

// Implement Embedded HAL traits so drivers (e.g., display drivers) accept this wrapper
impl<'a> embedded_hal::spi::SpiDevice for SpiDevice<'a> {
    fn transaction(&mut self, operations: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), self::Error> {
        self.inner.transaction(operations)
    }
}

impl<'a> embedded_hal::spi::ErrorType for SpiDevice<'a> {
    type Error = <SpiRefCellDevice<'a, Spi<'static, Blocking>, AnyPin<'static>, Delay> as embedded_hal::spi::ErrorType>::Error;
}

/// Wrapper for a device on a shared I2C bus.
pub struct I2cDevice<'a> {
    inner: I2cRefCellDevice<'a, I2c<'static, Blocking>>,
}

impl<'a> I2cDevice<'a> {
    /// Create a new device sharing the I2C bus
    pub fn new(bus: &'a RefCell<I2c<'static, Blocking>>) -> Self {
        let dev = I2cRefCellDevice::new(bus);
        Self { inner: dev }
    }
}

impl<'a> embedded_hal::i2c::I2c for I2cDevice<'a> {
    fn transaction(&mut self, address: u8, operations: &mut [embedded_hal::i2c::Operation<'_>]) -> Result<(), self::Error> {
        self.inner.transaction(address, operations)
    }
}

impl<'a> embedded_hal::i2c::ErrorType for I2cDevice<'a> {
    type Error = <I2cRefCellDevice<'a, I2c<'static, Blocking>> as embedded_hal::i2c::ErrorType>::Error;
}


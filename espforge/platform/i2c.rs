use esp_hal::{
    gpio::AnyPin,
    i2c::master::{Config, I2c},
    peripherals::I2C0,
    time::Rate,
    Blocking,
};
use embedded_hal::i2c::{ErrorType, I2c as EmbeddedI2c, Operation};

/// User-friendly I2C Master wrapper
///
/// Wraps the specific ESP-HAL driver and implements `embedded_hal` traits
/// so it can be consumed by device drivers (like SSD1306, MPU6050, etc).
pub struct I2CMaster {
    i2c: I2c<'static, Blocking>,
}

impl I2CMaster {
    /// Creates a new I2C Master with the specified SDA and SCL pins
    ///
    /// # Arguments
    /// * `sda` - The GPIO pin number for SDA
    /// * `scl` - The GPIO pin number for SCL
    /// * `frequency_khz` - The bus frequency in kHz
    ///
    /// # Panics
    /// Panics if the pins are invalid or if I2C0 is not available
    pub fn new(sda: u8, scl: u8, frequency_khz: u32) -> Self {
        // Safety: We ensure only one instance exists by consuming the AnyPin
        // In the context of generated code, this is called once per defined bus.
        let i2c0 = unsafe { I2C0::steal() };
        let sda_pin = unsafe { AnyPin::steal(sda) };
        let scl_pin = unsafe { AnyPin::steal(scl) };

        let config = Config::default().with_frequency(Rate::from_khz(frequency_khz));

        let i2c = I2c::new(i2c0, config)
            .unwrap()
            .with_sda(sda_pin)
            .with_scl(scl_pin);

        I2CMaster { i2c }
    }

    // --- Inherent methods for simple internal use or legacy scripts ---

    /// Writes bytes to the specified address
    pub fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), esp_hal::i2c::master::Error> {
        self.i2c.write(address, bytes)
    }

    /// Reads bytes from the specified address
    pub fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), esp_hal::i2c::master::Error> {
        self.i2c.read(address, buffer)
    }

    /// Writes bytes and then reads bytes from the specified address
    pub fn write_read(&mut self, address: u8, write_buffer: &[u8], read_buffer: &mut [u8]) -> Result<(), esp_hal::i2c::master::Error> {
        self.i2c.write_read(address, write_buffer, read_buffer)
    }
}

// --- embedded-hal Implementation ---
// This allows this struct to be passed to external drivers (like ssd1306)

impl ErrorType for I2CMaster {
    type Error = esp_hal::i2c::master::Error;
}

impl EmbeddedI2c for I2CMaster {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        // We delegate the transaction implementation directly to the underlying ESP-HAL driver,
        // which already implements embedded-hal 1.0.0
        // Use the trait method explicitly to resolve type mismatch with inherent method
        EmbeddedI2c::transaction(&mut self.i2c, address, operations)
    }
}


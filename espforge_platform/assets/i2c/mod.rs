#![allow(unexpected_cfgs)]
use esp_hal::{
    gpio::AnyPin,
    i2c::master::{Config, I2c},
    time::Rate,
    Blocking,
};
use embedded_hal::i2c::{ErrorType, I2c as EmbeddedI2c, Operation};

/// User-friendly I2C Master wrapper.
///
/// Wraps the specific ESP-HAL driver and implements `embedded_hal` traits.
pub struct I2CMaster {
    // We use 'static here because the driver is created from stolen peripherals,
    // which effectively live forever. This simplifies the high-level component types.
    i2c: I2c<'static, Blocking>,
}

impl I2CMaster {
    /// Creates a new I2C Master with the specified parameters.
    ///
    /// # Arguments
    /// * `i2c_num` - The I2C bus number (0 or 1).
    /// * `sda` - The GPIO pin number for SDA.
    /// * `scl` - The GPIO pin number for SCL.
    /// * `frequency_khz` - The bus frequency in kHz.
    ///
    /// # Panics
    /// Panics if `i2c_num` is invalid for the selected chip or if configuration fails.
    pub fn new(i2c_num: u8, sda: u8, scl: u8, frequency_khz: u32) -> Self {
        // Safety: We ensure only one instance exists by consuming the AnyPin via steal
        let sda_pin = unsafe { AnyPin::steal(sda) };
        let scl_pin = unsafe { AnyPin::steal(scl) };

        let config = Config::default().with_frequency(Rate::from_khz(frequency_khz));

        // Dynamically select the peripheral based on the bus number.
        // We use fully qualified paths for peripherals to avoid "unused import" or "unresolved item"
        // errors in rust-analyzer when compiling for chips that don't have I2C1 (like C3).
        let i2c_driver: I2c<'static, Blocking> = match i2c_num {
            0 => {
                let peripheral = unsafe { esp_hal::peripherals::I2C0::steal() };
                I2c::new(peripheral, config).unwrap()
            },
            // Enable I2C1 only for chips that have it (ESP32, S2, S3, H2, C6)
            #[cfg(any(feature = "esp32", feature = "esp32s2", feature = "esp32s3", feature = "esp32h2", feature = "esp32c6"))]
            1 => {
                let peripheral = unsafe { esp_hal::peripherals::I2C1::steal() };
                I2c::new(peripheral, config).unwrap()
            },
            _ => panic!("Invalid or unsupported I2C bus number: {}", i2c_num),
        };

        // Attach pins after creation
        let i2c = i2c_driver
            .with_sda(sda_pin)
            .with_scl(scl_pin);

        I2CMaster { i2c }
    }

    // /// Writes bytes to the specified address.
    // pub fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), esp_hal::i2c::master::Error> {
    //     self.i2c.write(address, bytes)
    // }

    // /// Reads bytes from the specified address.
    // pub fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), esp_hal::i2c::master::Error> {
    //     self.i2c.read(address, buffer)
    // }

    // /// Writes bytes and then reads bytes from the specified address.
    // pub fn write_read(&mut self, address: u8, write_buffer: &[u8], read_buffer: &mut [u8]) -> Result<(), esp_hal::i2c::master::Error> {
    //     self.i2c.write_read(address, write_buffer, read_buffer)
    // }
}


impl ErrorType for I2CMaster {
    type Error = esp_hal::i2c::master::Error;
}

impl EmbeddedI2c for I2CMaster {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        EmbeddedI2c::transaction(&mut self.i2c, address, operations)
    }
}

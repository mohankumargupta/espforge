#![allow(unexpected_cfgs)]
use esp_hal::{
    gpio::AnyPin,
    i2c::master::{Config, I2c},
    time::Rate,
    Blocking,
};
use embedded_hal::i2c::{I2c as I2cTrait, ErrorType, Operation};

pub struct I2CMaster {
    i2c: I2c<'static, Blocking>,
}

impl I2CMaster {
    pub fn new(i2c_num: u8, sda: u8, scl: u8, frequency_khz: u32) -> Self {
        // ... (Keep your existing initialization logic here) ...
        let sda_pin = unsafe { AnyPin::steal(sda) };
        let scl_pin = unsafe { AnyPin::steal(scl) };

        let config = Config::default().with_frequency(Rate::from_khz(frequency_khz));

        let i2c_driver = match i2c_num {
            0 => {
                let peripheral = unsafe { esp_hal::peripherals::I2C0::steal() };
                I2c::new(peripheral, config).unwrap()
            },
            #[cfg(any(feature = "esp32", feature = "esp32s2", feature = "esp32s3", feature = "esp32h2"))]
            1 => {
                let peripheral = unsafe { esp_hal::peripherals::I2C1::steal() };
                I2c::new(peripheral, config).unwrap()
            },
            _ => panic!("Invalid I2C bus number: {}", i2c_num),
        };

        Self {
            i2c: i2c_driver.with_sda(sda_pin).with_scl(scl_pin)
        }
    }
    
    // You can remove the manual write/read/write_read methods if you want,
    // as the trait implementation below provides them automatically.
}

// 1. Implement ErrorType
impl ErrorType for I2CMaster {
    type Error = esp_hal::i2c::master::Error;
}

// 2. Implement I2c Trait
// Note: esp-hal I2C supports u8 (7-bit) addresses.
impl I2cTrait<u8> for I2CMaster {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        self.i2c.transaction(address, operations)
    }
}


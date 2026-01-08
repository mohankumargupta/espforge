use crate::platform::i2c::I2CMaster;
use core::cell::RefCell;
use embedded_hal::i2c::{ErrorType, I2c, Operation};

pub struct I2C {
    master: RefCell<I2CMaster>,
}

impl I2C {
    pub fn new(i2c_bus: u8, sda: u8, scl: u8, frequency: u32) -> Self {
        I2C {
            // Default to 100kHz standard mode
            master: RefCell::new(I2CMaster::new(i2c_bus, sda, scl, frequency)),
        }
    }

    /// Consumes this component wrapper and returns the underlying Platform Driver.
    /// 
    /// This is used by "Devices" (e.g., Display Drivers) that need ownership 
    /// of the generic `embedded-hal` I2C interface.
    pub fn into_inner(self) -> I2CMaster {
        self.master.into_inner()
    }
}

impl ErrorType for I2C {
    type Error = esp_hal::i2c::master::Error;
}

impl I2c for I2C {
    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        self.master.borrow_mut().transaction(address, operations)
    }
}


use crate::platform::i2c::I2CMaster;
use core::cell::RefCell;

pub struct I2C {
    master: RefCell<I2CMaster>,
}

impl I2C {
    pub fn new(sda: u8, scl: u8) -> Self {
        I2C {
            // Default to 100kHz standard mode
            master: RefCell::new(I2CMaster::new(sda, scl, 100)),
        }
    }

    /// Probes an I2C address to see if a device acknowledges it.
    /// Useful for I2C scanning.
    pub fn probe(&self, address: u8) -> bool {
        let mut buffer = [0u8; 1];
        // Attempt to read 1 byte. If successful, the device exists.
        self.master.borrow_mut().read(address, &mut buffer).is_ok()
    }
    
    pub fn write(&self, address: u8, data: u8) {
        let _ = self.master.borrow_mut().write(address, &[data]);
    }

    /// Consumes this component wrapper and returns the underlying Platform Driver.
    /// 
    /// This is used by "Devices" (e.g., Display Drivers) that need ownership 
    /// of the generic `embedded-hal` I2C interface.
    pub fn into_inner(self) -> I2CMaster {
        self.master.into_inner()
    }
}
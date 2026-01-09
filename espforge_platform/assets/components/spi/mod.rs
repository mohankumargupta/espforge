use crate::platform::spi::SPIMaster;
use core::cell::RefCell;
use embedded_hal::spi::{SpiBus, ErrorType};

pub struct SPI<'a> {
    bus: &'a RefCell<SPIMaster>,
}

impl<'a> SPI<'a> {
    pub fn new(bus: &'a RefCell<SPIMaster>) -> Self {
        Self { bus }
    }
}

impl<'a> ErrorType for SPI<'a> {
    type Error = esp_hal::spi::Error;
}

impl<'a> SpiBus for SPI<'a> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().read(words)
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().write(words)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().transfer(read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().transfer_in_place(words)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.bus.borrow_mut().flush()
    }
}


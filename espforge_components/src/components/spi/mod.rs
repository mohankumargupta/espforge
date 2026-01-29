use core::cell::RefCell;
use espforge_platform::esp_hal::spi::master::Spi;
use espforge_platform::esp_hal::Blocking;
use embedded_hal::spi::{ErrorType, SpiBus};

// Re-export the config
pub use espforge_common::components::spi::SpiDeviceConfig;

#[derive(Clone, Copy)]
pub struct SPI<'a> {
    bus: &'a RefCell<Spi<'static, Blocking>>,
}

impl<'a> SPI<'a> {
    pub fn new(bus: &'a RefCell<Spi<'static, Blocking>>) -> Self {
        Self { bus }
    }

    pub fn bus(&self) -> &'a RefCell<Spi<'static, Blocking>> {
        self.bus
    }
}

impl<'a> ErrorType for SPI<'a> {
    type Error = espforge_platform::esp_hal::spi::Error;
}

impl<'a> SpiBus for SPI<'a> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().read(words)
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().write(words)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        SpiBus::transfer(&mut *self.bus.borrow_mut(), read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.bus.borrow_mut().transfer_in_place(words)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.bus.borrow_mut().flush()
    }
}

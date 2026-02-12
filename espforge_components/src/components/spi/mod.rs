use core::cell::RefCell;
use embedded_hal::spi::{ErrorType, SpiBus};
use espforge_platform::esp_hal::Blocking;
use espforge_platform::esp_hal::spi::master::Spi;

// Re-export the config
pub use espforge_common::components::spi::SpiDeviceConfig;

#[derive(Clone, Copy)]
pub struct SPI {
    bus: &'static RefCell<Spi<'static, Blocking>>,
}

impl SPI {
    pub fn new(bus: &'static RefCell<Spi<'static, Blocking>>) -> Self {
        Self { bus }
    }

    pub fn bus(&self) -> &'static RefCell<Spi<'static, Blocking>> {
        self.bus
    }
}

impl ErrorType for SPI {
    type Error = espforge_platform::esp_hal::spi::Error;
}

impl SpiBus for SPI {
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

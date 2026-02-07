#![no_std]

pub mod logger;
pub mod delay;
pub mod gpio;
#[cfg(any(feature = "spi", feature = "i2c"))]
pub mod bus;
#[cfg(feature = "i2c")]
pub mod i2c;
#[cfg(feature = "spi")]
pub mod spi;
#[cfg(feature = "uart")]
pub mod uart;

#[cfg(feature = "embassy")]
pub mod signal;

pub use esp_hal;

 
pub struct Context {
    pub logger: logger::Logger,
    pub delay: delay::Delay,
}

impl Context {
    pub fn new() -> Self {
        Self {
            logger: logger::Logger::new(),
            delay: delay::Delay::new(),
        }
    }
}


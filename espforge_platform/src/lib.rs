#![no_std]

#[cfg(any(feature = "spi", feature = "i2c"))]
pub mod bus;
pub mod delay;
pub mod gpio;
#[cfg(feature = "i2c")]
pub mod i2c;
pub mod logger;
#[cfg(feature = "spi")]
pub mod spi;
#[cfg(feature = "uart")]
pub mod uart;
#[cfg(feature = "wifi")]
pub mod wifi;
#[cfg(feature = "wifi")]
pub use embassy_net;
#[cfg(feature = "embassy")]
pub mod signal;

pub use esp_hal;

pub struct Context {
    pub logger: logger::Logger,
    pub delay: delay::Delay,
    #[cfg(feature = "wifi")]
    pub wifi: wifi::WifiClient,
}

impl Context {
    #[cfg(not(feature = "wifi"))]
    pub fn new() -> Self {
        Self {
            logger: logger::Logger::new(),
            delay: delay::Delay::new(),
        }
    }
}

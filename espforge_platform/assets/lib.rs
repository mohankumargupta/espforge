pub mod logger;
pub mod delay;
pub mod components;
pub mod gpio; 
pub mod bus;
pub mod i2c;
pub mod spi;
pub mod uart;

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


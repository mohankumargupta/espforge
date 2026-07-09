//! Runtime implementations of Component capabilities (hardware-backed or
//! software-service): Led, I2c, ...

pub mod i2c;
pub mod led;

pub use i2c::I2cBus;
pub use led::Led;


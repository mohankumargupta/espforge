//! Runtime implementations of Component capabilities (hardware-backed or
//! software-service): Led, I2c, ...
//!
//! Modules and re-exports are gated by their `espforge-runtime` feature
//! (design §19.1) so a project compiles only the capabilities it uses.

#[cfg(feature = "i2c")]
pub mod i2c;
#[cfg(feature = "led")]
pub mod led;

#[cfg(feature = "i2c")]
pub use i2c::I2cBus;
#[cfg(feature = "led")]
pub use led::Led;

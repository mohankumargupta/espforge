//! Runtime implementations of Component capabilities (hardware-backed or
//! software-service): Led, I2c, ...
//!
//! Modules and re-exports are gated by their `espforge-runtime` feature
//! (design §19.1) so a project compiles only the capabilities it uses.

#[cfg(feature = "i2c")]
pub mod i2c;
#[cfg(feature = "led")]
pub mod led;
#[cfg(feature = "button")]
pub mod button;
#[cfg(feature = "spi")]
pub mod spi;
#[cfg(feature = "uart")]
pub mod uart;

#[cfg(feature = "i2c")]
pub use i2c::I2cBus;
#[cfg(feature = "led")]
pub use led::Led;
#[cfg(feature = "button")]
pub use button::Button;
#[cfg(feature = "spi")]
pub use spi::SpiBus;
#[cfg(feature = "uart")]
pub use uart::UartDevice;

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
pub use i2c::{I2cBus, I2cConfig, I2cError};
#[cfg(feature = "led")]
pub use led::Led;
#[cfg(feature = "button")]
pub use button::Button;
#[cfg(feature = "spi")]
pub use spi::{SpiBus, SpiConfig, SpiDevice, SpiError};
#[cfg(feature = "uart")]
pub use uart::{UartConfig, UartDevice, UartError};
// Software-service components are re-exported here so the generated
// `ctx.components.http` accessor is uniform with hardware components (ADR-013).
// Re-exported from `services/`, where they actually live.
#[cfg(feature = "http")]
pub use crate::services::Http;

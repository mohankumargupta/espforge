// GPIO Module for espforge_platform
//
// This module provides GPIO abstractions for both blocking and async (Embassy) modes.
// 
// - In blocking mode: Uses standard Input/Output types
// - In async mode: Uses Flex-based types with interrupt-driven async operations
//
// The module structure ensures clean separation between blocking and async implementations.

#[cfg(not(feature = "embassy"))]
mod blocking;

#[cfg(not(feature = "embassy"))]
pub use blocking::{GPIOInput, GPIOOutput};

#[cfg(feature = "embassy")]
pub mod blocking;

#[cfg(feature = "embassy")]
mod embassy;

#[cfg(feature = "embassy")]
pub use embassy::{GPIOInput, GPIOOutput};

// Re-export AnyPin for convenience
pub use esp_hal::gpio::AnyPin;


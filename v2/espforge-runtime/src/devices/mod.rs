//! Runtime implementations of terminal Devices: Ssd1306, Ili9341, ...
//!
//! Module gated by its `espforge-runtime` feature (design §19.1).

#[cfg(feature = "ssd1306")]
pub mod ssd1306;
#[cfg(feature = "ili9341")]
pub mod ili9341;

#[cfg(feature = "ssd1306")]
pub use ssd1306::Ssd1306;
#[cfg(feature = "ili9341")]
pub use ili9341::Ili9341;

//! Runtime implementations of terminal Devices: Ssd1306, Ili9341, ...
//!
//! Module gated by its `espforge-runtime` feature (design §19.1).

#[cfg(feature = "ssd1306")]
pub mod ssd1306;

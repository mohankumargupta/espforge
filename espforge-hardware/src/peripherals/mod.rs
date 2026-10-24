//! `# peripherals` lowering: one module per bus/GPIO kind.
//!
//! Each module turns one `<gpio>`, `<i2c>`, `<spi>` or `<uart>` node into the
//! pins and settings the board claims. The `<*_component>` branches live in
//! [`crate::board`].

pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;

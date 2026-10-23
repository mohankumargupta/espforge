//! `# peripherals` lowering: one module per bus/GPIO kind.
//!
//! Each module owns its draft type(s) plus the `lower_*` branch of the
//! `# peripherals` loop and the `finish_*` filter that drops bare buses.
//! The `<*_component>` branches live in [`crate::components`]; drafts are
//! shared between the two halves.

pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;

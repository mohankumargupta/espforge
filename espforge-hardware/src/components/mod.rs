//! `# components` lowering: one module per component kind.
//!
//! Each module owns the `attach_*` branch of the `# components` loop. The
//! `$parent` lookup goes through the typed wrapper in `espforge-bindings`
//! (the sole consumer of the context handles); pin/slug/occupancy checks
//! stay here so messages stay byte-identical. Draft tables live in
//! [`crate::peripherals`].

pub mod button;
pub mod i2c_component;
pub mod led;
pub mod spi_component;
pub mod uart_component;

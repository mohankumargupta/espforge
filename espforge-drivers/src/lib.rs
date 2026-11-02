//! espforge-drivers: device drivers for generated firmware projects.
//!
//! Drivers wrap third-party crates and expose the types and operations app
//! code uses, so a generated project never names the vendor crate directly.
//! They depend on [`espforge_runtime`] for the bus handles and delays the
//! generated `Components` struct hands out.
//!
//! Target-only in practice, but the crate compiles on the host too (the
//! esp-hal parts live behind one cfg), so `cargo build`/`cargo test` on the
//! workspace work.

#![cfg_attr(not(test), no_std)]

/// The `# devices` half of the schema, embedded from beside the drivers it
/// describes.
///
/// `espforge-schema` parses it; `espforge-elements` composes it with
/// `espforge_schema::SCHEMA_KDL`, so a new driver never edits a compiler crate.
/// The constant lives here, in the crate whose package contains the file, so
/// both stay publishable.
pub const DRIVERS_SCHEMA_KDL: &str = include_str!("../drivers.schema.kdl");

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ssd1306"
))]
pub mod ssd1306;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ssd1306"
))]
pub use ssd1306::*;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ili9341"
))]
pub mod ili9341;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "ili9341"
))]
pub use ili9341::*;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "scd4x"
))]
pub mod scd4x;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "scd4x"
))]
pub use scd4x::*;

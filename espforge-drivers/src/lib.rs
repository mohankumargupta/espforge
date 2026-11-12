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

/// The bundled driver manifests, one file per tag, embedded from beside the
/// drivers they describe.
///
/// The label is the path a user can open; `espforge-elements` composes these
/// with the core schema at runtime and in its build script. A new bundled
/// driver is one file plus this list, plus its feature and module.
pub const MANIFESTS: &[(&str, &str)] = &[
    (
        "espforge-drivers/manifests/ssd1306.kdl",
        include_str!("../manifests/ssd1306.kdl"),
    ),
    (
        "espforge-drivers/manifests/ili9341.kdl",
        include_str!("../manifests/ili9341.kdl"),
    ),
    (
        "espforge-drivers/manifests/scd4x.kdl",
        include_str!("../manifests/scd4x.kdl"),
    ),
    (
        "espforge-drivers/manifests/adxl345.kdl",
        include_str!("../manifests/adxl345.kdl"),
    ),
];

/// The concatenation of [`MANIFESTS`], for callers that take one schema text.
///
/// `espforge-schema::load_all` accepts several sources, so new code should use
/// [`MANIFESTS`] instead; the constant keeps the old one-string entry point.
pub const DRIVERS_SCHEMA_KDL: &str = concat!(
    include_str!("../manifests/ssd1306.kdl"),
    "\n",
    include_str!("../manifests/ili9341.kdl"),
    "\n",
    include_str!("../manifests/scd4x.kdl"),
    "\n",
    include_str!("../manifests/adxl345.kdl"),
    "\n",
);

/// Every bundled tag, in manifest order.
pub const TAGS: &[&str] = &["ssd1306", "ili9341", "scd4x", "adxl345"];

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

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "adxl345"
))]
pub mod adxl345;

#[cfg(all(
    any(target_arch = "riscv32", target_arch = "xtensa"),
    feature = "adxl345"
))]
pub use adxl345::*;

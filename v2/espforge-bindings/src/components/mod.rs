//! In-tree component driver implementations (ADR-006). Each component driver is
//! one module that declares its `Driver` trait impl, including the `construct`
//! snippet that tells the emitter how to wire the instance move-by-value
//! (ADR-008). This is the single-declaration model: adding a component driver =
//! one file here + a catalog entry.

mod i2c;
mod led;

use espforge_model::driver::Registry;

/// The explicit, in-tree driver registry (ADR-006). No `inventory` / link-time
/// discovery — the CLI indexes this by `kind`.
pub fn registry() -> Registry {
    Registry::new(&[&led::LED, &i2c::I2C])
}

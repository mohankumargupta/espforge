//! espforge-bindings: in-tree driver catalog (validation metadata) + driver
//! registry (codegen impls), per ADR-006.

pub mod catalog;
pub mod components;
pub mod devices;

pub use catalog::catalog;

use espforge_model::driver::Registry;

/// The explicit, in-tree driver registry (ADR-006): the union of all component
/// and device drivers, indexed by `kind`. Tier-aware grouping happens in the
/// `components` / `devices` submodules; this merged view is what the emitter
/// and validator index against.
pub fn registry() -> Registry {
    let mut drivers = components::registry().all().to_vec();
    drivers.extend(devices::registry().all().iter().copied());
    Registry::new(&drivers)
}

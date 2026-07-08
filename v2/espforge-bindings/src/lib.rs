//! espforge-bindings: in-tree driver catalog (validation metadata) + driver
//! registry (codegen impls), per ADR-006.

pub mod catalog;
pub mod drivers;

pub use catalog::catalog;
pub use drivers::registry;

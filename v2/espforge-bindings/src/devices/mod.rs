//! In-tree device driver implementations (ADR-006). Terminal high-level drivers
//! that consume shared components by reference (ADR-003/008). Each device driver
//! is one module that declares its `Driver` trait impl, including the `construct`
//! snippet that tells the emitter how to wire the instance move-by-value.

mod ssd1306;

use espforge_model::driver::Registry;

/// The explicit, in-tree device driver registry (ADR-006).
pub fn registry() -> Registry {
    Registry::new(&[&ssd1306::SSD1306])
}

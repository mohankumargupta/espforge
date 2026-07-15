//! In-tree driver catalog (ADR-006): the data-only metadata used to validate and
//! resolve projects (ADR-009). Code-generation (`Driver` trait impls) is added in
//! a later phase; this crate currently owns the *specs* so `validate`/`resolve`
//! can reason about known kinds, tiers, and dependency shapes.

use espforge_model::catalog::{DepSpec, DriverSpec, SpecFlags};
use espforge_model::ir::{Access, DepKind, Tier};

/// The set of built-in drivers, keyed by `kind`.
///
/// Validation metadata only:
/// - `led`        — Component, claims one pin by value.
/// - `i2c`        — Component, claims an I2C bus peripheral by value.
/// - `ssd1306`    — Device, shares an `i2c` component by reference (bus sharing
///                 lives at the component tier, ADR-003/008) and claims a few
///                 control pins by value.
/// - `button`     — Component, claims one pin by value (an input with optional
///                 pull-up).
pub fn catalog() -> Vec<DriverSpec> {
    vec![
        DriverSpec {
            kind: "led".to_string(),
            tier: Tier::Component,
            deps: vec![],
            pins: vec!["pin".to_string()],
            peripherals: vec![],
            flags: SpecFlags { needs_delay: true, ..Default::default() },
        },
        DriverSpec {
            kind: "i2c".to_string(),
            tier: Tier::Component,
            deps: vec![],
            pins: vec![],
            peripherals: vec!["bus".to_string()],
            flags: SpecFlags::default(),
        },
        DriverSpec {
            kind: "ssd1306".to_string(),
            tier: Tier::Device,
            deps: vec![DepSpec {
                key: "component".to_string(),
                kind: DepKind::Instance,
                access: Access::Shared,
            }],
            pins: vec![],
            peripherals: vec![],
            flags: SpecFlags::default(),
        },
        DriverSpec {
            kind: "button".to_string(),
            tier: Tier::Component,
            deps: vec![],
            pins: vec!["pin".to_string()],
            peripherals: vec![],
            flags: SpecFlags::default(),
        },
        DriverSpec {
            kind: "spi".to_string(),
            tier: Tier::Component,
            deps: vec![],
            pins: vec![],
            peripherals: vec!["bus".to_string()],
            flags: SpecFlags::default(),
        },
        DriverSpec {
            kind: "uart".to_string(),
            tier: Tier::Component,
            deps: vec![],
            pins: vec![],
            peripherals: vec!["bus".to_string()],
            flags: SpecFlags::default(),
        },
        DriverSpec {
            kind: "ili9341".to_string(),
            tier: Tier::Device,
            deps: vec![DepSpec {
                key: "spi".to_string(),
                kind: DepKind::Instance,
                access: Access::Shared,
            }],
            pins: vec!["dc".to_string(), "rst".to_string(), "cs".to_string()],
            peripherals: vec![],
            flags: SpecFlags { needs_delay: true, ..Default::default() },
        },
        // `http` is a software-service component (ADR-012): it claims no
        // peripheral and takes no `with:` bus — it consumes the implicit Stack
        // built from `esp32.wifi`. It forces Embassy + network stack + alloc.
        DriverSpec {
            kind: "http".to_string(),
            tier: Tier::Component,
            deps: vec![],
            pins: vec![],
            peripherals: vec![],
            flags: SpecFlags {
                is_embassy: true,
                has_wifi: true,
                needs_stack: true,
                has_alloc: true,
                ..Default::default()
            },
        },
    ]
}

//! Shared codegen helpers for driver `construct` impls (ADR-006).
//!
//! These are *stateless* and backend-independent, so they are free functions,
//! not `Backend` methods: every driver needs them identically. The backend-
//! specific rendering (how a pin becomes an `Output`, how a ctor call reads)
//! lives in [`crate::backend::Backend`] instead.

use crate::driver::GenContext;
use crate::ir::{Peripheral, ResolvedInstance};
use crate::value::Diag;
use serde_yaml_ng::Value;

/// Sanitize an instance id into a valid Rust identifier (used for the
/// `Components`/`Devices` struct field name).
pub fn sanitize(id: &str) -> String {
    let mut out = String::new();
    for (i, c) in id.chars().enumerate() {
        if c.is_alphanumeric() && (i == 0 && c.is_alphabetic() || i > 0) {
            out.push(c);
        } else if i > 0 {
            out.push('_');
        }
    }
    if out.is_empty() {
        out = "inst".into();
    }
    out
}

/// Resolve the esp_hal field name for a claimed peripheral.
pub fn peripheral_field(peripherals: &[Peripheral], name: &str) -> String {
    peripherals
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.field.clone())
        .unwrap_or_else(|| "unreachable!()".to_string())
}

/// Resolve a pin named by a `with` key (e.g. `reset`) to its esp_hal GPIO
/// field specifier, e.g. `GPIO16`.
///
/// The `with` value is a resource ref (`$gpio16`) to a declared gpio
/// peripheral, whose `field` (`GPIO16`) is already resolved by the validate
/// stage. Resolving through the peripheral table keeps this correct for any pin
/// declaration syntax — it does not string-parse `$GPIO` literals (ADR-008).
/// Missing pins render `unreachable!()` like the other unresolved fallbacks.
pub fn gpio_field_from_with(ctx: &GenContext, inst: &ResolvedInstance, key: &str) -> String {
    inst.with
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix('$'))
        .and_then(|name| ctx.peripherals.iter().find(|p| p.name == name))
        .map(|p| p.field.clone())
        .unwrap_or_else(|| "unreachable!()".to_string())
}

/// Bool from a `with` key, defaulting to `false` when absent.
pub fn bool_or(inst: &ResolvedInstance, key: &str, default: bool) -> bool {
    inst.with.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// Build a `Diag` for an unexpected construction situation.
pub fn diag(msg: impl Into<String>) -> Diag {
    Diag::error(msg)
}

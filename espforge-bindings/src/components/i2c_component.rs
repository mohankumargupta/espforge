//! Typed wrapper for the `<i2c_component>` bus component.
//!
//!   `<i2c_component id="temp_bus" i2c="$i2c0" address="0x48" />`
//!
//! Presence, the `0x..`-or-decimal address kind and the `$`-reference target
//! come from `espf.schema.kdl`, generated into
//! [`schema::Attrs`](crate::components::generated_elements::i2c_component).
//! This module owns the reference lookup and the wording of the errors.

use espf::context::{ActivePeripheral, Handle, ParsedNode, RuntimeContext};

use crate::components::attrs::Issue;
use crate::components::generated_elements::i2c_component as schema;

/// A fully validated `<i2c_component>` node.
#[derive(Debug, Clone)]
pub struct I2cComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `i2c` attribute (e.g. `"$i2c0"`).
    pub i2c_ref: &'a str,
    /// Parsed `address` attribute (e.g. `0x48` -> `0x48`).
    pub address: u8,
    /// Handle of the referenced parent i2c peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::I2c`).
    pub peripheral: ActivePeripheral,
}

impl<'a> I2cComponent<'a> {
    /// Validates and resolves an i2c_component node against the runtime context.
    pub fn resolve(node: &'a ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let attrs = schema::Attrs::from_node(node).map_err(|issue| message(node.id, issue))?;
        let id = attrs.id.value;
        let address = attrs.address.value;
        let name = attrs.i2c.name;

        let handle = ctx.interner.get(name).ok_or_else(|| {
            format!("❌ [i2c_component] '{id}' references undefined i2c peripheral '{name}'")
        })?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| {
                format!("❌ [i2c_component] '{id}' i2c peripheral '{name}' is not active")
            })?;

        if !matches!(peripheral, ActivePeripheral::I2c { .. }) {
            return Err(format!(
                "❌ [i2c_component] '{id}' expects an i2c peripheral, got a non-I2C parent bus"
            ));
        }

        // Same slug rule `hardware` enforces so the wrapper can replace the
        // manual `$` lookup without changing the user-visible text.
        let slug = id.replace('-', "_");
        if !is_rust_ident(&slug) {
            return Err(format!(
                "<i2c_component id=\"{id}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)"
            ));
        }

        Ok(Self {
            id,
            i2c_ref: attrs.i2c.raw,
            address,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

/// This element's wording for a schema-level problem. `0x..` and decimal
/// addresses still get their own sentence, because that is what users see
/// today (`espforge/tests/diagnostics`).
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [i2c_component] Missing required attribute '{attr}'")
        }
        Issue::Missing { attr } => {
            format!("❌ [i2c_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid { value, .. } if value.starts_with("0x") || value.starts_with("0X") => {
            format!("❌ [i2c_component] '{id}' invalid hex address '{value}'")
        }
        Issue::Invalid { value, .. } => {
            format!("❌ [i2c_component] '{id}' invalid decimal address '{value}'")
        }
    }
}

fn is_rust_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

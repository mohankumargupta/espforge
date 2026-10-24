//! Typed wrapper for the `<gpio_component>` bus component.
//!
//!   `<gpio_component id="blue_led" gpio="$gpio4" direction="output" />`
//!
//! Which attributes exist, which are required and which `direction` values are
//! allowed is read from `espf.schema.kdl` and generated into
//! [`schema::Attrs`](crate::components::generated_elements::gpio_component).
//! This module owns the `$` reference lookup and the wording of the errors.

use espf::context::{ActivePeripheral, Handle, ParsedNode, RuntimeContext};

use crate::components::attrs::Issue;
use crate::components::generated_elements::gpio_component as schema;

/// Valid values for the `direction` attribute, generated from the schema's
/// `choices` list.
pub use crate::components::generated_elements::gpio_component::Direction as GpioDirection;

/// A fully validated `<gpio_component>` node.
#[derive(Debug, Clone)]
pub struct GpioComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `gpio` attribute (e.g. `"$gpio4"`).
    pub gpio_ref: &'a str,
    /// Parsed `direction` attribute.
    pub direction: GpioDirection,
    /// Handle of the referenced parent gpio peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Gpio`).
    pub peripheral: ActivePeripheral,
}

impl<'a> GpioComponent<'a> {
    /// Validates and resolves a gpio_component node against the runtime context.
    pub fn resolve(node: &'a ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let attrs = schema::Attrs::from_node(node).map_err(|issue| message(node.id, issue))?;
        let id = attrs.id.value;
        let name = attrs.gpio.name;

        // Resolve the `$peripheral` reference through the context tables,
        // exactly like drivers do (integer-offset lookup, no string searching
        // after resolution).
        let handle = ctx.interner.get(name).ok_or_else(|| {
            format!("❌ [gpio_component] '{id}' references undefined gpio peripheral '{name}'")
        })?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| {
                format!("❌ [gpio_component] '{id}' gpio peripheral '{name}' is not active")
            })?;

        if !matches!(peripheral, ActivePeripheral::Gpio) {
            return Err(format!(
                "❌ [gpio_component] '{id}' expects a gpio peripheral, got a non-GPIO parent bus"
            ));
        }

        Ok(Self {
            id,
            gpio_ref: attrs.gpio.raw,
            direction: attrs.direction.value,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

/// This element's wording for a schema-level problem. The message is not
/// generated: it is the text users already see, and it moves to a diagnostic
/// code as a unit (extreme.md §3.4).
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [gpio_component] Missing required attribute '{attr}'")
        }
        Issue::Missing { attr } => {
            format!("❌ [gpio_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid { attr, value } => format!(
            "❌ [gpio_component] Invalid {attr} '{value}': expected \"output\" or \"input\""
        ),
    }
}

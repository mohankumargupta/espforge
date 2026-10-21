//! Typed wrapper for the `<gpio_component>` bus component.
//!
//! Schema (from `examples/example.espf.txt`):
//!   `<gpio_component id="blue_led" gpio="$gpio4" direction="output" />`
//!
//! Required attributes: `id`, `gpio` (a `$`-prefixed reference to a `<gpio>`
//! peripheral), `direction` (`output` | `input`).

use espf::context::{ActivePeripheral, Handle, RuntimeContext};

/// Valid values for the `direction` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioDirection {
    Output,
    Input,
}

impl GpioDirection {
    /// Parses the `direction` attribute value.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "output" => Ok(Self::Output),
            "input" => Ok(Self::Input),
            other => Err(format!(
                "❌ [gpio_component] Invalid direction '{other}': expected \"output\" or \"input\""
            )),
        }
    }
}

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
    pub fn resolve(node: &'a espf::context::ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let id = node.id;
        if id.is_empty() {
            return Err("❌ [gpio_component] Missing required attribute 'id'".to_string());
        }

        let gpio_ref = node
            .get_attr("gpio")
            .ok_or_else(|| format!("❌ [gpio_component] '{id}' missing required attribute 'gpio'"))?;

        let direction_raw = node
            .get_attr("direction")
            .ok_or_else(|| format!("❌ [gpio_component] '{id}' missing required attribute 'direction'"))?;
        let direction = GpioDirection::parse(direction_raw)?;

        // Resolve the `$peripheral` reference through the context tables,
        // exactly like drivers do (integer-offset lookup, no string searching
        // after resolution).
        let name = gpio_ref.trim_start_matches('$');
        let handle = ctx
            .interner
            .get(name)
            .ok_or_else(|| format!("❌ [gpio_component] '{id}' references undefined gpio peripheral '{name}'"))?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| format!("❌ [gpio_component] '{id}' gpio peripheral '{name}' is not active"))?;

        if !matches!(peripheral, ActivePeripheral::Gpio) {
            return Err(format!(
                "❌ [gpio_component] '{id}' expects a gpio peripheral, got a non-GPIO parent bus"
            ));
        }

        Ok(Self {
            id,
            gpio_ref,
            direction,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

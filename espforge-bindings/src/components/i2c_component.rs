//! Typed wrapper for the `<i2c_component>` bus component.
//!
//! Schema (from `examples/example.espf.txt`):
//!   `<i2c_component id="temp_bus" i2c="$i2c0" address="0x48" />`
//!
//! Required attributes: `id`, `i2c` (a `$`-prefixed reference to an `<i2c>`
//! peripheral), `address` (hex or decimal, e.g. `0x48`).

use espf::context::{ActivePeripheral, Handle, RuntimeContext};

/// The attributes `resolve` requires besides `id`. Drift-checked against
/// the `espforge espf` catalog.
pub const REQUIRED_ATTRS: &[&str] = &["i2c", "address"];

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
    pub fn resolve(node: &'a espf::context::ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let id = node.id;
        if id.is_empty() {
            return Err("❌ [i2c_component] Missing required attribute 'id'".to_string());
        }

        let i2c_ref = node
            .get_attr("i2c")
            .ok_or_else(|| format!("❌ [i2c_component] '{id}' missing required attribute 'i2c'"))?;

        let address_raw = node
            .get_attr("address")
            .ok_or_else(|| format!("❌ [i2c_component] '{id}' missing required attribute 'address'"))?;

        // Accept both `0x48` (hex) and plain decimal, matching the lenient
        // behaviour of `RuntimeContext::resolve_and_bind`.
        let address = parse_address(id, address_raw)?;

        let name = i2c_ref.trim_start_matches('$');
        let handle = ctx
            .interner
            .get(name)
            .ok_or_else(|| format!("❌ [i2c_component] '{id}' references undefined i2c peripheral '{name}'"))?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| format!("❌ [i2c_component] '{id}' i2c peripheral '{name}' is not active"))?;

        if !matches!(peripheral, ActivePeripheral::I2c { .. }) {
            return Err(format!(
                "❌ [i2c_component] '{id}' expects an i2c peripheral, got a non-I2C parent bus"
            ));
        }

        Ok(Self {
            id,
            i2c_ref,
            address,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

/// Parses an I2C device address: `0x48`-style hex or decimal.
pub fn parse_address(component_id: &str, raw: &str) -> Result<u8, String> {
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
            .map_err(|_| format!("❌ [i2c_component] '{component_id}' invalid hex address '{raw}'"))
    } else {
        raw.parse::<u8>()
            .map_err(|_| format!("❌ [i2c_component] '{component_id}' invalid decimal address '{raw}'"))
    }
}

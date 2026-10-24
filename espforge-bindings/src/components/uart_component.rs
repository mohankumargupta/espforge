//! Typed wrapper for the `<uart_component>` bus component.
//!
//!   `<uart_component id="serial_bus" uart="$uart1" baud_rate="9600" parity="none" stop_bits="1" />`
//!
//! Presence and the framing `choices` come from `espf.schema.kdl`, generated
//! into [`schema::Attrs`](crate::components::generated_elements::uart_component).
//! This module owns the reference lookup and the wording of the errors.

use espf::context::{ActivePeripheral, Handle, ParsedNode, RuntimeContext};

use crate::components::attrs::Issue;
use crate::components::generated_elements::uart_component as schema;

/// Valid values for the `parity` attribute, generated from the schema.
pub use crate::components::generated_elements::uart_component::Parity;

/// Valid values for the `stop_bits` attribute, generated from the schema.
pub use crate::components::generated_elements::uart_component::StopBits;

/// A fully validated `<uart_component>` node.
#[derive(Debug, Clone)]
pub struct UartComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `uart` attribute (e.g. `"$uart1"`).
    pub uart_ref: &'a str,
    /// Parsed `baud_rate` attribute.
    pub baud_rate: u32,
    /// Parsed `parity` attribute (defaults to `Parity::None`).
    pub parity: Parity,
    /// Parsed `stop_bits` attribute (defaults to `StopBits::N1`).
    pub stop_bits: StopBits,
    /// Handle of the referenced parent uart peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Uart`).
    pub peripheral: ActivePeripheral,
}

impl<'a> UartComponent<'a> {
    /// Validates and resolves a uart_component node against the runtime context.
    pub fn resolve(node: &'a ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let attrs = schema::Attrs::from_node(node).map_err(|issue| message(node.id, issue))?;
        let id = attrs.id.value;
        let name = attrs.uart.name;

        let handle = ctx.interner.get(name).ok_or_else(|| {
            format!("❌ [uart_component] '{id}' references undefined uart peripheral '{name}'")
        })?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| {
                format!("❌ [uart_component] '{id}' uart peripheral '{name}' is not active")
            })?;

        if !matches!(peripheral, ActivePeripheral::Uart) {
            return Err(format!(
                "❌ [uart_component] '{id}' expects a uart peripheral, got a non-UART parent bus"
            ));
        }

        Ok(Self {
            id,
            uart_ref: attrs.uart.raw,
            baud_rate: attrs.baud_rate.value,
            parity: attrs.parity.value,
            stop_bits: attrs.stop_bits.value,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

/// This element's wording for a schema-level problem.
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [uart_component] Missing required attribute '{attr}'")
        }
        Issue::Missing { attr } => {
            format!("❌ [uart_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid {
            attr: "baud_rate",
            value,
        } => format!("❌ [uart_component] '{id}' invalid baud_rate '{value}'"),
        Issue::Invalid {
            attr: "parity",
            value,
        } => format!(
            "❌ [uart_component] Invalid parity '{value}': expected \"none\", \"even\" or \"odd\""
        ),
        Issue::Invalid { value, .. } => {
            format!("❌ [uart_component] Invalid stop_bits '{value}': expected \"1\" or \"2\"")
        }
    }
}

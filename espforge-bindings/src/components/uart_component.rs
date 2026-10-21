//! Typed wrapper for the `<uart_component>` bus component.
//!
//! Schema (from `examples/example.espf.txt`):
//!   `<uart_component id="serial_bus" uart="$uart1" baud_rate="9600" parity="none" stop_bits="1" />`
//!
//! Required attributes: `id`, `uart` (a `$`-prefixed reference to a `<uart>`
//! peripheral), `baud_rate`. Optional: `parity` (`none` | `even` | `odd`),
//! `stop_bits` (`1` | `2`).

use espf::context::{ActivePeripheral, Handle, RuntimeContext};

/// Valid values for the `parity` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

impl Parity {
    /// Parses the `parity` attribute value.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "even" => Ok(Self::Even),
            "odd" => Ok(Self::Odd),
            other => Err(format!(
                "❌ [uart_component] Invalid parity '{other}': expected \"none\", \"even\" or \"odd\""
            )),
        }
    }
}

/// Valid values for the `stop_bits` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

impl StopBits {
    /// Parses the `stop_bits` attribute value.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "1" => Ok(Self::One),
            "2" => Ok(Self::Two),
            other => Err(format!(
                "❌ [uart_component] Invalid stop_bits '{other}': expected \"1\" or \"2\""
            )),
        }
    }
}

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
    /// Parsed `stop_bits` attribute (defaults to `StopBits::One`).
    pub stop_bits: StopBits,
    /// Handle of the referenced parent uart peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Uart`).
    pub peripheral: ActivePeripheral,
}

impl<'a> UartComponent<'a> {
    /// Validates and resolves a uart_component node against the runtime context.
    pub fn resolve(node: &'a espf::context::ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let id = node.id;
        if id.is_empty() {
            return Err("❌ [uart_component] Missing required attribute 'id'".to_string());
        }

        let uart_ref = node
            .get_attr("uart")
            .ok_or_else(|| format!("❌ [uart_component] '{id}' missing required attribute 'uart'"))?;

        let baud_raw = node
            .get_attr("baud_rate")
            .ok_or_else(|| format!("❌ [uart_component] '{id}' missing required attribute 'baud_rate'"))?;
        let baud_rate: u32 = baud_raw
            .parse()
            .map_err(|_| format!("❌ [uart_component] '{id}' invalid baud_rate '{baud_raw}'"))?;

        let parity = match node.get_attr("parity") {
            Some(p) => Parity::parse(p)?,
            None => Parity::None,
        };

        let stop_bits = match node.get_attr("stop_bits") {
            Some(s) => StopBits::parse(s)?,
            None => StopBits::One,
        };

        let name = uart_ref.trim_start_matches('$');
        let handle = ctx
            .interner
            .get(name)
            .ok_or_else(|| format!("❌ [uart_component] '{id}' references undefined uart peripheral '{name}'"))?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| format!("❌ [uart_component] '{id}' uart peripheral '{name}' is not active"))?;

        if !matches!(peripheral, ActivePeripheral::Uart) {
            return Err(format!(
                "❌ [uart_component] '{id}' expects a uart peripheral, got a non-UART parent bus"
            ));
        }

        Ok(Self {
            id,
            uart_ref,
            baud_rate,
            parity,
            stop_bits,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

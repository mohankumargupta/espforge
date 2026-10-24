//! `<uart>` peripheral lowering.
//!
//! The `<uart_component>` branch lives in [`crate::board`]; this module only
//! turns one `<uart>` node into its peripheral name and two pins.

use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_bindings::components::attrs::Issue;
use espforge_bindings::components::generated_elements::uart as schema;

#[derive(Debug, Clone)]
pub struct UartPeripheral {
    /// The `<uart>` id, which is also the peripheral id unless `uart="UARTn"`
    /// overrides it.
    pub id: String,
    pub peripheral: String,
    pub tx: u8,
    pub rx: u8,
}

/// The implicit way to name a `<uart>`'s physical peripheral: `uart0` ->
/// `UART0`, `uart1` -> `UART1`, ...; anything else is the user's error (the
/// esp-hal peripheral structs are named exactly this).
pub fn uart_peripheral_name(n: &ParsedNode) -> Result<String, EspfError> {
    let id = n.id;
    if matches!(id, "uart0" | "uart1" | "uart2" | "uart3") {
        return Ok(id.to_ascii_uppercase());
    }
    Err(EspfError::at_node(
        n,
        "invalid uart id",
        format!(
            "<uart id=\"{id}\">: the id must be uart0, uart1, uart2 or uart3 (or give uart=\"UART1\" explicitly)"
        ),
    ))
}

/// An explicit `uart="UART1"` attribute on `<uart>`: the physical
/// `Peripherals.UART*` peripheral, case-insensitive. With it, the `<uart>`
/// id stops being encoded as `uartN` and is a plain reference name.
pub fn uart_attr_peripheral(n: &ParsedNode, raw: &str) -> Result<String, EspfError> {
    let upper = raw.trim().to_ascii_uppercase();
    if matches!(upper.as_str(), "UART0" | "UART1" | "UART2" | "UART3") {
        return Ok(upper);
    }
    Err(EspfError::at_node(
        n,
        "invalid uart peripheral",
        format!(
            "<uart id=\"{}\"> uart=\"{raw}\" must be UART0, UART1, UART2 or UART3",
            n.id
        ),
    ))
}

/// The `<uart>` branch of the `# peripherals` loop.
pub fn lower_uart(n: &ParsedNode) -> Result<UartPeripheral, EspfError> {
    let attrs = schema::Attrs::from_node(n)
        .map_err(|issue| EspfError::at_node(n, "invalid uart pin", message(n.id, issue)))?;
    let peripheral = match attrs.uart {
        // Explicit physical peripheral (`uart="UART1"`); the id is
        // then just a reference name.
        Some(raw) => uart_attr_peripheral(n, raw.value)?,
        // Backwards compatible: the id itself names the peripheral.
        None => uart_peripheral_name(n)?,
    };
    Ok(UartPeripheral {
        id: n.id.to_string(),
        peripheral,
        tx: attrs.tx.value,
        rx: attrs.rx.value,
    })
}

/// This element's wording for a schema-level problem.
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => {
            format!("<uart id=\"{id}\"> needs a numeric {attr}=\"...\"")
        }
    }
}

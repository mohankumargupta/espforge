//! `<uart>` peripheral lowering.
//!
//! The `<uart_component>` branch lives in
//! [`crate::components::uart_component`]; the draft below is shared
//! between the two.

use crate::UartSpec;
use espf::context::ParsedNode;
use espf::diag::EspfError;

/// The `<uart_component>` riding on a `<uart>`: its fields are what the
/// generated code ends up with (framing lives on the component).
#[derive(Debug, Clone)]
pub struct UartCompDraft {
    pub name: String,
    pub baud: u32,
    pub parity: String,
    pub stop_bits: String,
}

#[derive(Debug, Clone)]
pub struct UartDraft {
    /// The `<uart>` id, which is also the peripheral id unless `uart="UARTn"`
    /// overrides it.
    pub id: String,
    pub peripheral: String,
    pub tx: u8,
    pub rx: u8,
    /// The component riding on this uart, if any.
    pub component: Option<UartCompDraft>,
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
pub fn lower_uart(n: &ParsedNode) -> Result<UartDraft, EspfError> {
    let peripheral = match n.get_attr("uart") {
        // Explicit physical peripheral (`uart="UART1"`); the id is
        // then just a reference name.
        Some(raw) => uart_attr_peripheral(n, raw)?,
        // Backwards compatible: the id itself names the peripheral.
        None => uart_peripheral_name(n)?,
    };
    let pin_attr = |what: &str| -> Result<u8, EspfError> {
        n.get_attr(what).and_then(|p| p.parse::<u8>().ok()).ok_or_else(|| {
            EspfError::at_node(
                n,
                "invalid uart pin",
                format!("<uart id=\"{}\"> needs a numeric {what}=\"...\"", n.id),
            )
        })
    };
    Ok(UartDraft {
        id: n.id.to_string(),
        peripheral,
        tx: pin_attr("tx")?,
        rx: pin_attr("rx")?,
        component: None,
    })
}

/// A bare `<uart>` with no component generates nothing: the app can only
/// reach the uart through a component anyway.
pub fn finish_uart(uarts: Vec<UartDraft>) -> Vec<UartSpec> {
    uarts
        .into_iter()
        .filter_map(|u| {
            u.component.map(|c| UartSpec {
                name: c.name,
                peripheral: u.peripheral,
                tx: u.tx,
                rx: u.rx,
                baud: c.baud,
                parity: c.parity,
                stop_bits: c.stop_bits,
            })
        })
        .collect()
}

//! `<uart>` peripheral + `<uart_component>` lowering (step 1d).
//!
//! Moved verbatim from `espforge/src/commands/pipeline.rs::hardware`: the
//! `uart_peripheral_name` / `uart_attr_peripheral` helpers, the `<uart>`
//! peripheral branch, and the `<uart_component>` component branch.

use super::{UartSpec, is_rust_ident};
use espf::context::ParsedNode;

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
pub fn uart_peripheral_name(id: &str) -> Result<String, String> {
    if matches!(id, "uart0" | "uart1" | "uart2" | "uart3") {
        return Ok(id.to_ascii_uppercase());
    }
    Err(format!(
        "<uart id=\"{id}\">: the id must be uart0, uart1, uart2 or uart3 (or give uart=\"UART1\" explicitly)"
    ))
}

/// An explicit `uart="UART1"` attribute on `<uart>`: the physical
/// `Peripherals.UART*` peripheral, case-insensitive. With it, the `<uart>`
/// id stops being encoded as `uartN` and is a plain reference name.
pub fn uart_attr_peripheral(raw: &str, id: &str) -> Result<String, String> {
    let upper = raw.trim().to_ascii_uppercase();
    if matches!(upper.as_str(), "UART0" | "UART1" | "UART2" | "UART3") {
        return Ok(upper);
    }
    Err(format!(
        "<uart id=\"{id}\"> uart=\"{raw}\" must be UART0, UART1, UART2 or UART3"
    ))
}

/// The `<uart>` branch of the `# peripherals` loop.
pub fn lower_uart(n: &ParsedNode) -> Result<UartDraft, String> {
    let peripheral = match n.get_attr("uart") {
        // Explicit physical peripheral (`uart="UART1"`); the id is
        // then just a reference name.
        Some(raw) => uart_attr_peripheral(raw, n.id)?,
        // Backwards compatible: the id itself names the peripheral.
        None => uart_peripheral_name(n.id)?,
    };
    let pin_attr = |what: &str| {
        n.get_attr(what)
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| format!("<uart id=\"{}\"> needs a numeric {what}=\"...\"", n.id))
    };
    Ok(UartDraft {
        id: n.id.to_string(),
        peripheral,
        tx: pin_attr("tx")?,
        rx: pin_attr("rx")?,
        component: None,
    })
}

/// The `<uart_component>` branch of the `# components` loop.
pub fn attach_uart_component(
    n: &ParsedNode,
    uarts: &mut [UartDraft],
) -> Result<(), String> {
    let uart_ref = n.get_attr("uart").ok_or_else(|| {
        format!("<uart_component id=\"{}\"> needs uart=\"$<uart id>\"", n.id)
    })?;
    let target = uart_ref.trim_start_matches('$');
    let draft = uarts
        .iter_mut()
        .find(|u| u.id == target)
        .ok_or_else(|| {
            format!(
                "<uart_component id=\"{}\"> refers to `{target}`, which is not an <uart> in `# peripherals`",
                n.id
            )
        })?;
    if let Some(other) = &draft.component {
        return Err(format!(
            "<uart_component id=\"{}\"> and <uart_component id=\"{}\"> both use <uart id=\"{target}\">",
            other.name,
            n.id.replace('-', "_")
        ));
    }
    let baud_raw = n.get_attr("baud_rate").ok_or_else(|| {
        format!("<uart_component id=\"{}\"> needs baud_rate=\"...\"", n.id)
    })?;
    let baud: u32 = baud_raw.parse().map_err(|_| {
        format!(
            "<uart_component id=\"{}\"> baud_rate=\"{baud_raw}\" is not a number",
            n.id
        )
    })?;
    if baud == 0 {
        return Err(format!(
            "<uart_component id=\"{}\"> baud_rate=\"{baud_raw}\" must be greater than 0",
            n.id
        ));
    }
    let parity = match n.get_attr("parity") {
        None | Some("none") => "None".to_string(),
        Some("even") => "Even".to_string(),
        Some("odd") => "Odd".to_string(),
        Some(other) => {
            return Err(format!(
                "<uart_component id=\"{}\"> parity=\"{other}\" must be \"none\", \"even\" or \"odd\"",
                n.id
            ));
        }
    };
    let stop_bits = match n.get_attr("stop_bits") {
        None | Some("1") => "_1".to_string(),
        Some("2") => "_2".to_string(),
        Some(other) => {
            return Err(format!(
                "<uart_component id=\"{}\"> stop_bits=\"{other}\" must be \"1\" or \"2\"",
                n.id
            ));
        }
    };
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(format!(
            "<uart_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
            n.id
        ));
    }
    draft.component = Some(UartCompDraft {
        name,
        baud,
        parity,
        stop_bits,
    });
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::super::lower;
    use super::*;

    const WITH_UART: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"6\" rx=\"5\" />\n# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<super::super::Hardware, String> {
        lower(&sections(src))
    }

    #[test]
    fn a_uart_with_a_component_becomes_hardware() {
        let hw = lower_src(WITH_UART).unwrap();
        assert_eq!(
            hw.uarts,
            [UartSpec {
                name: "serial".into(),
                peripheral: "UART1".into(),
                tx: 6,
                rx: 5,
                baud: 9600,
                parity: "None".into(),
                stop_bits: "_1".into(),
            }]
        );
        // Framing attributes flow through too.
        let src = WITH_UART.replace(
            "baud_rate=\"9600\"",
            "baud_rate=\"115200\" parity=\"even\" stop_bits=\"2\"",
        );
        let spec = &lower_src(&src).unwrap().uarts[0];
        assert_eq!(
            (spec.baud, spec.parity.as_str(), spec.stop_bits.as_str()),
            (115200, "Even", "_2")
        );
    }

    #[test]
    fn a_bare_uart_generates_nothing() {
        let src = WITH_UART.replace(
            "# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n",
            "# components\n",
        );
        assert!(lower_src(&src).unwrap().uarts.is_empty());
    }

    #[test]
    fn uart_wiring_conflicts_are_refused() {
        // tx and rx on the same pin.
        let err = lower_src(&WITH_UART.replace("tx=\"6\" rx=\"5\"", "tx=\"5\" rx=\"5\""))
            .unwrap_err();
        assert!(err.contains("tx and rx"), "{err}");

        // A second uart sharing the rx pin.
        let src = WITH_UART.replace(
            "rx=\"5\" />\n# components",
            "rx=\"5\" />\n  <uart id=\"uart2\" tx=\"7\" rx=\"5\" />\n# components",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("both use GPIO5"), "{err}");

        // A led on the tx pin.
        let src = WITH_UART
            .replace(
                "tx=\"6\" rx=\"5\" />\n# components",
                "tx=\"6\" rx=\"5\" />\n  <gpio id=\"gpio6\" pin=\"6\" direction=\"output\" />\n# components",
            )
            .replace(
                "# components\n",
                "# components\n  <led id=\"l\" gpio=\"$gpio6\" />\n",
            );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("both use GPIO6"), "{err}");
    }

    #[test]
    fn a_uart_can_select_its_peripheral_explicitly_and_frees_the_id() {
        // `uart="UART1"` names the physical peripheral; the id is then just a
        // reference name.
        let src = WITH_UART
            .replace(
                "<uart id=\"uart1\" tx=\"6\" rx=\"5\" />",
                "<uart id=\"gps\" uart=\"UART1\" tx=\"6\" rx=\"5\" />",
            )
            .replace("uart=\"$uart1\"", "uart=\"$gps\"");
        let spec = &lower_src(&src).unwrap().uarts[0];
        assert_eq!(spec.peripheral, "UART1");

        // Lowercase is accepted too (the id spelling no longer matters).
        let src = WITH_UART
            .replace("uart=\"$uart1\"", "uart=\"$gps\"")
            .replace(
                "<uart id=\"uart1\" tx=\"6\" rx=\"5\" />",
                "<uart id=\"gps\" uart=\"uart1\" tx=\"6\" rx=\"5\" />",
            );
        assert_eq!(lower_src(&src).unwrap().uarts[0].peripheral, "UART1");

        // A nonsensical peripheral name is refused.
        let bad = WITH_UART.replace("tx=\"6\" rx=\"5\"", "uart=\"UART9\" tx=\"6\" rx=\"5\"");
        let err = lower_src(&bad).unwrap_err();
        assert!(err.contains("must be UART0, UART1, UART2 or UART3"), "{err}");

        // Without the attribute the old rule still applies: the id must be
        // `uart0`..`uart3`.
        let src = WITH_UART
            .replace("<uart id=\"uart1\"", "<uart id=\"gps\"")
            .replace("uart=\"$uart1\"", "uart=\"$gps\"");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("the id must be uart0, uart1, uart2 or uart3"), "{err}");
    }

    #[test]
    fn two_uarts_on_one_peripheral_are_refused() {
        // Both via explicit `uart=` attributes...
        let src = WITH_UART.replace(
            "rx=\"5\" />\n# components",
            "rx=\"5\" />\n  <uart id=\"other\" uart=\"UART1\" tx=\"7\" rx=\"8\" />\n# components",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(
            err.contains("<uart id=\"uart1\">") && err.contains("both use UART1"),
            "{err}"
        );

        // ...and when one derives it from its id.
        let src = WITH_UART.replace(
            "rx=\"5\" />\n# components",
            "rx=\"5\" />\n  <uart id=\"gps\" uart=\"uart1\" tx=\"7\" rx=\"8\" />\n# components",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("both use UART1"), "{err}");
    }
}

//! `<uart_component>` lowering.
//!
//! The `<uart>` draft lives in [`crate::peripherals::uart`]; this module
//! owns the `<uart_component>` branch of the `# components` loop.

use crate::is_rust_ident;
use crate::peripherals::uart::{UartCompDraft, UartDraft};
use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_bindings::components::{Parity, StopBits, UartComponent};

/// The `<uart_component>` branch of the `# components` loop.
///
/// Step 2c: the `$` parent lookup goes through the typed wrapper
/// (`UartComponent::resolve`), the sole consumer of the context handles.
/// Baud/framing/slug/occupancy checks stay here so messages stay byte-identical.
pub fn attach_uart_component(
    n: &ParsedNode,
    uarts: &mut [UartDraft],
    ctx: &RuntimeContext,
) -> Result<(), EspfError> {
    // Single dispatch path for the `$` ref; wrapper owns `trim_start_matches`.
    let comp = UartComponent::resolve(n, ctx)
        .map_err(|m| EspfError::at_node(n, "invalid uart_component", m))?;
    // Locate the draft via the resolved handle, not a string `find`.
    let idx = uarts
        .iter()
        .position(|u| ctx.interner.get(&u.id) == Some(comp.peripheral_handle))
        .ok_or_else(|| {
            let target = comp.uart_ref.trim_start_matches('$');
            EspfError::at_node(
                n,
                "unknown uart",
                format!(
                    "<uart_component id=\"{}\"> refers to `{target}`, which is not an <uart> in `# peripherals`",
                    n.id
                ),
            )
        })?;
    let target = uarts[idx].id.clone();
    let draft = &mut uarts[idx];
    if let Some(other) = &draft.component {
        return Err(EspfError::at_node(
            n,
            "uart already used",
            format!(
                "<uart_component id=\"{}\"> and <uart_component id=\"{}\"> both use <uart id=\"{target}\">",
                other.name,
                n.id.replace('-', "_")
            ),
        ));
    }
    // Baud and framing come from the wrapper, which coerced them from the
    // schema and already rejected a malformed value; zero is the only rule
    // hardware adds on top.
    let baud = comp.baud_rate;
    if baud == 0 {
        return Err(EspfError::at_node(
            n,
            "invalid baud_rate",
            format!(
                "<uart_component id=\"{}\"> baud_rate=\"0\" must be greater than 0",
                n.id
            ),
        ));
    }
    let parity = parity_variant(comp.parity).to_string();
    let stop_bits = stop_bits_variant(comp.stop_bits).to_string();
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(EspfError::at_node(
            n,
            "invalid component id",
            format!(
                "<uart_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ),
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

/// The `esp_hal::uart::Parity` variant a schema `parity` choice names.
///
/// Exhaustive on purpose: a new spelling in `espf.schema.kdl` must be decided
/// here rather than silently defaulting.
fn parity_variant(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "None",
        Parity::Even => "Even",
        Parity::Odd => "Odd",
    }
}

/// The `esp_hal::uart::StopBits` variant a schema `stop_bits` choice names.
fn stop_bits_variant(stop_bits: StopBits) -> &'static str {
    match stop_bits {
        StopBits::N1 => "_1",
        StopBits::N2 => "_2",
    }
}

#[cfg(test)]
mod tests {
    use crate::UartSpec;
    use crate::lower_sections;

    const WITH_UART: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"6\" rx=\"5\" />\n# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<crate::Hardware, String> {
        lower_sections(&sections(src))
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
    fn a_zero_baud_rate_is_refused() {
        // The wrapper coerces `baud_rate` from the schema; zero is the one
        // rule hardware adds, so it stays here.
        let src = WITH_UART.replace("baud_rate=\"9600\"", "baud_rate=\"0\"");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("must be greater than 0"), "{err}");
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
        let err =
            lower_src(&WITH_UART.replace("tx=\"6\" rx=\"5\"", "tx=\"5\" rx=\"5\"")).unwrap_err();
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
        assert!(
            err.contains("must be UART0, UART1, UART2 or UART3"),
            "{err}"
        );

        // Without the attribute the old rule still applies: the id must be
        // `uart0`..`uart3`.
        let src = WITH_UART
            .replace("<uart id=\"uart1\"", "<uart id=\"gps\"")
            .replace("uart=\"$uart1\"", "uart=\"$gps\"");
        let err = lower_src(&src).unwrap_err();
        assert!(
            err.contains("the id must be uart0, uart1, uart2 or uart3"),
            "{err}"
        );
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

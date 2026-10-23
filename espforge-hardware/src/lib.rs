//! Hardware lowering: validated sections -> [`Hardware`].
//!
//! Per-bus lowering lives in [`peripherals`] (drafts + the `# peripherals`
//! loop) and [`components`] (the `# components` branches), services in
//! [`services`], pin/peripheral clashes in [`conflicts`]. This module is the
//! thin orchestrator: check [`devices`], dispatch the `# peripherals` /
//! `# components` loops in spec order (so the first error in file order
//! wins, exactly like the old `pipeline::hardware`), run
//! [`conflicts::check_all`], then assemble [`Hardware`]
//!
//! `lower` takes the parsed sections map instead of the CLI's `Checked` so
//! this crate stays a pure library (`espforge` -> `espforge-hardware`, not
//! the reverse). `pipeline::hardware` is a one-line wrapper over it.
//!
//! ```text
//! mod peripherals::{gpio, i2c, spi, uart}
//! mod components::{led, button, i2c_component, spi_component, uart_component}
//! mod services::wifi
//! mod devices  // not buildable yet: rejects any entry
//! mod conflicts, util
//! pub fn lower(sections, ctx) -> Result<Hardware, Vec<EspfError>>
//! pub fn buildable(sections, ctx) -> Result<(), Vec<EspfError>>
//! ```
//!
//! Item 2e: `buildable` is `lower(...).map(|_| ())` — the single validation
//! path is `lower`; there is no separate buildability check.

pub mod components;
pub mod conflicts;
pub mod devices;
pub mod peripherals;
pub mod services;
pub mod util;

use components::{button, i2c_component, led, spi_component, uart_component};
use peripherals::{gpio, i2c, spi, uart};
use services::wifi;

// Re-export so lowering modules and callers share one import path.
pub use espforge_model::hardware::*;

use std::collections::HashMap;

use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;

/// The hardware and services `build` can generate code for today: `<gpio>`
/// peripherals used by `<led>` and `<button>` components, `<i2c>` peripherals
/// used by `<i2c_component>` components, `<spi>` peripherals used by
/// `<spi_component>` components, and one `<wifi>` service. Anything else is a
/// clear "not supported yet".
pub fn lower(
    sections: &HashMap<String, Vec<ParsedNode>>,
    ctx: &RuntimeContext,
) -> Result<Hardware, Vec<EspfError>> {
    devices::reject_if_present(sections)?;

    let mut gpios: Vec<gpio::GpioEntry> = Vec::new();
    let mut i2c_buses: Vec<i2c::I2cBusDraft> = Vec::new();
    let mut spi_buses: Vec<spi::SpiBusDraft> = Vec::new();
    let mut uarts: Vec<uart::UartDraft> = Vec::new();

    for n in sections
        .get("peripherals")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if n.tag_name == "gpio" {
            gpios.push(gpio::lower_gpio(n).map_err(|e| vec![e])?);
        } else if n.tag_name == "i2c" {
            i2c_buses.push(i2c::lower_i2c(n).map_err(|e| vec![e])?);
        } else if n.tag_name == "spi" {
            spi_buses.push(spi::lower_spi(n).map_err(|e| vec![e])?);
        } else if n.tag_name == "uart" {
            uarts.push(uart::lower_uart(n).map_err(|e| vec![e])?);
        } else {
            return Err(vec![EspfError::at_node(
                n,
                "unsupported peripheral",
                format!(
                    "<{}> in `# peripherals` is not supported by `build` yet (only <gpio>, <i2c>, <spi> and <uart>)",
                    n.tag_name
                ),
            )]);
        }
    }

    let mut spi_devices: Vec<spi::SpiDeviceDraft> = Vec::new();
    let mut leds: Vec<LedSpec> = Vec::new();
    let mut buttons: Vec<ButtonSpec> = Vec::new();

    for n in sections
        .get("components")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if n.tag_name == "i2c_component" {
            i2c_component::attach_i2c_component(n, &mut i2c_buses, ctx)
                .map_err(|e| vec![e])?;
        } else if n.tag_name == "spi_component" {
            spi_component::attach_spi_component(
                n,
                &mut spi_buses,
                &mut spi_devices,
                &gpios,
                ctx,
            )
                .map_err(|e| vec![e])?;
        } else if n.tag_name == "uart_component" {
            uart_component::attach_uart_component(n, &mut uarts, ctx).map_err(|e| vec![e])?;
        } else if n.tag_name == "button" {
            button::attach_button(n, &gpios, &leds, &mut buttons, ctx).map_err(|e| vec![e])?;
        } else if n.tag_name == "led" {
            led::attach_led(n, &gpios, &mut leds, &buttons, ctx).map_err(|e| vec![e])?;
        } else {
            return Err(vec![EspfError::at_node(
                n,
                "unsupported component",
                format!(
                    "<{}> in `# components` is not supported by `build` yet (only <led>, <button>, <i2c_component>, <spi_component> and <uart_component>)",
                    n.tag_name
                ),
            )]);
        }
    }

    let wifi = wifi::lower_services(
        sections
            .get("services")
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )?;

    conflicts::check_all(
        sections,
        &gpios,
        &i2c_buses,
        &spi_buses,
        &spi_devices,
        &uarts,
        &leds,
        &buttons,
    )?;

    let i2cs = i2c::finish_i2c(i2c_buses);
    let (spi_bus_specs, spis) = spi::finish_spi(spi_buses, spi_devices);
    let uart_specs = uart::finish_uart(uarts);

    Ok(Hardware {
        leds,
        buttons,
        i2cs,
        spi_buses: spi_bus_specs,
        spis,
        uarts: uart_specs,
        wifi,
    })
}

/// Whether `build` can turn this spec into firmware.
pub fn buildable(
    sections: &HashMap<String, Vec<ParsedNode>>,
    ctx: &RuntimeContext,
) -> Result<(), Vec<EspfError>> {
    lower(sections, ctx).map(|_| ())
}

/// Test helper: `lower` over a sections map. Builds a bound
/// `RuntimeContext` via `resolve_and_bind` first, so tests exercise the
/// same triangulation `pipeline::validate` feeds into `lower`.
/// Errors are joined to a single `String` (labels concatenated) so existing
/// per-bus tests keep asserting on message text unchanged.
pub fn lower_sections(sections: &HashMap<String, Vec<ParsedNode>>) -> Result<Hardware, String> {
    lower_spans(sections).map_err(|errs| {
        errs
            .into_iter()
            .map(|e| format!("{}: {}", e.title, e.label))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// Span-preserving variant of [`lower_sections`] for snippet tests.
pub fn lower_spans(
    sections: &HashMap<String, Vec<ParsedNode>>,
) -> Result<Hardware, Vec<EspfError>> {
    let mut ctx = RuntimeContext::new();
    ctx.resolve_and_bind(
        sections.get("peripherals").map(Vec::as_slice).unwrap_or(&[]),
        sections.get("components").map(Vec::as_slice).unwrap_or(&[]),
        sections.get("devices").map(Vec::as_slice).unwrap_or(&[]),
    )
    .map_err(|errs| errs)?;
    lower(sections, &ctx)
}

/// Snapshot baseline for item 2e (wiring errors as snippets).
///
/// Each test records the *current* `String` error text (the "before") and
/// shows the *same text* rendered as an `EspfError` snippet (the "after").
/// When `lower` switches to `Vec<EspfError>`, the `label` stays identical
/// and only the rendering changes from `❌ {msg}` to a file/line snippet.
#[cfg(test)]
mod snapshot_tests {
    use super::lower_sections;
    use espf::diag::{EspfError, render_errors};
    use std::collections::HashMap;

    fn sections_of(src: &str) -> HashMap<String, Vec<espf::context::ParsedNode<'_>>> {
        espf::parser::parse_espf(src).sections
    }

    fn component_node<'a>(
        sections: &'a HashMap<String, Vec<espf::context::ParsedNode<'a>>>,
        id: &str,
    ) -> &'a espf::context::ParsedNode<'a> {
        sections
            .get("components")
            .unwrap()
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("no component {id}"))
    }

    fn renders_as_snippet(src: &str, node_id: &str, label: &str) {
        let parsed = espf::parser::parse_espf(src);
        let node = parsed.sections.get("components").unwrap().iter().find(|n| n.id == node_id).unwrap();
        let err = EspfError::at_node(node, "invalid hardware", label);
        let out = render_errors("snap.espf", src, &[err]);
        assert!(out.contains("snap.espf"), "{out}");
        assert!(out.contains(label), "{out}");
        // Snippet points at the offending `<tag` line, not a bare `❌` line.
        assert!(out.contains("error:"), "{out}");
    }

    const I2C_DOUBLE: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n  <i2c_component id=\"c2\" i2c=\"$i2c0\" address=\"0x49\" />\n# devices\n# services\n";

    #[test]
    fn i2c_double_use_snapshot() {
        let err = lower_sections(&sections_of(I2C_DOUBLE)).unwrap_err();
        assert!(err.contains("both use"), "{err}");
        renders_as_snippet(I2C_DOUBLE, "c2", &err);
        let _ = component_node(&sections_of(I2C_DOUBLE), "c2");
    }

    const LED_DOUBLE: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" />\n  <led id=\"other\" gpio=\"$gpio18\" />\n# devices\n# services\n";

    #[test]
    fn led_double_use_snapshot() {
        let err = lower_sections(&sections_of(LED_DOUBLE)).unwrap_err();
        assert!(err.contains("both use GPIO18"), "{err}");
        renders_as_snippet(LED_DOUBLE, "other", &err);
    }

    const SPI_CS_CLASH: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" />\n# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />\n  <spi_component id=\"second\" spi=\"$spi2\" cs=\"9\" />\n# devices\n# services\n";

    #[test]
    fn spi_cs_clash_snapshot() {
        let err = lower_sections(&sections_of(SPI_CS_CLASH)).unwrap_err();
        assert!(err.contains("both use GPIO9"), "{err}");
        renders_as_snippet(SPI_CS_CLASH, "second", &err);
    }

    const UART_TX_RX: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"5\" rx=\"5\" />\n# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n# devices\n# services\n";

    #[test]
    fn uart_tx_rx_snapshot() {
        let err = lower_sections(&sections_of(UART_TX_RX)).unwrap_err();
        assert!(err.contains("tx and rx"), "{err}");
        renders_as_snippet(UART_TX_RX, "serial", &err);
    }
}

/// Test helper: `buildable` over a sections map (mirrors the old
/// `pipeline::buildable(&Checked)` shape without the CLI type).
/// Builds a bound `RuntimeContext` via `resolve_and_bind` first, so tests
/// exercise the same triangulation `pipeline::validate` feeds into `lower`.
pub fn buildable_sections(sections: &HashMap<String, Vec<ParsedNode>>) -> Result<(), String> {
    buildable_spans(sections).map_err(|errs| {
        errs.into_iter()
            .map(|e| format!("{}: {}", e.title, e.label))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// Span-preserving variant of [`buildable_sections`].
pub fn buildable_spans(sections: &HashMap<String, Vec<ParsedNode>>) -> Result<(), Vec<EspfError>> {
    let mut ctx = RuntimeContext::new();
    ctx.resolve_and_bind(
        sections.get("peripherals").map(Vec::as_slice).unwrap_or(&[]),
        sections.get("components").map(Vec::as_slice).unwrap_or(&[]),
        sections.get("devices").map(Vec::as_slice).unwrap_or(&[]),
    )
    .map_err(|errs| errs)?;
    buildable(sections, &ctx)
}

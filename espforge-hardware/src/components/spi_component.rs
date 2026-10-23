//! `<spi_component>` lowering.
//!
//! The `<spi>` bus draft lives in [`crate::peripherals::spi`]; this module
//! owns the `<spi_component>` branch of the `# components` loop.

use crate::is_rust_ident;
use crate::peripherals::gpio::GpioEntry;
use crate::peripherals::spi::{SpiBusDraft, SpiDeviceDraft};
use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_bindings::components::SpiComponent;

/// The `<spi_component>` branch of the `# components` loop. Pushes into
/// `devices` and marks the bus used.
///
/// Step 2c: the `$` parent lookup goes through the typed wrapper
/// (`SpiComponent::resolve`), the sole consumer of the context handles.
/// `mode`/`frequency` stay on `<spi>` (examples/hardware); the wrapper treats
/// component `mode` as optional (default 0) and `cs` as numeric pin *or* gpio
/// id. Pin/duplicate checks stay here so messages stay byte-identical.
pub fn attach_spi_component(
    n: &ParsedNode,
    spi_buses: &mut [SpiBusDraft],
    spi_devices: &mut Vec<SpiDeviceDraft>,
    gpios: &[GpioEntry],
    ctx: &RuntimeContext,
) -> Result<(), EspfError> {
    // Single dispatch path for the `$` ref; wrapper owns `trim_start_matches`.
    // Wrapper accepts numeric `cs` and optional `mode`, so existing
    // `spi_example` specs (no mode, numeric cs) resolve here.
    let comp =
        SpiComponent::resolve(n, ctx).map_err(|m| EspfError::at_node(n, "invalid spi_component", m))?;
    let target = comp.spi_ref.trim_start_matches('$');
    let bus_idx = spi_buses.iter().position(|b| {
        ctx.interner.get(&b.id) == Some(comp.peripheral_handle)
    });
    if bus_idx.is_none() {
        return Err(EspfError::at_node(
            n,
            "unknown spi bus",
            format!(
                "<spi_component id=\"{}\"> refers to `{target}`, which is not an <spi> in `# peripherals`",
                n.id
            ),
        ));
    }
    // `cs="9"` is a pin; `cs="gpio5"`/`cs="$gpio5"` resolves via the gpio table.
    let cs_ref = comp.cs_ref.trim_start_matches('$');
    let cs: u8 = if let Ok(pin) = cs_ref.parse::<u8>() {
        pin
    } else if let Some(g) = gpios.iter().find(|g| g.id == cs_ref) {
        g.pin
    } else {
        return Err(EspfError::at_node(
            n,
            "invalid cs",
            format!("<spi_component id=\"{}\"> needs a numeric cs=\"...\"", n.id),
        ));
    };
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(EspfError::at_node(
            n,
            "invalid component id",
            format!(
                "<spi_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ),
        ));
    }
    if let Some(other) = spi_devices.iter().find(|d| d.cs == cs) {
        return Err(EspfError::at_node(
            n,
            "cs pin already used",
            format!(
                "<spi_component id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{cs}",
                other.name, name
            ),
        ));
    }
    let idx = bus_idx.expect("checked above");
    spi_buses[idx].used = true;
    let bus_id = spi_buses[idx].id.clone();
    spi_devices.push(SpiDeviceDraft {
        name,
        bus: bus_id,
        cs,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::lower_sections;
    use crate::{SpiBusSpec, SpiSpec};

    const WITH_SPI: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"100kHz\" />\n# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />\n# devices\n# services\n";

    const WITH_BUS: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<crate::Hardware, String> {
        lower_sections(&sections(src))
    }

    #[test]
    fn a_spi_component_with_a_bus_becomes_hardware() {
        let hw = lower_src(WITH_SPI).unwrap();
        assert_eq!(
            hw.spi_buses,
            [SpiBusSpec {
                id: "spi2".into(),
                peripheral: "SPI2".into(),
                sclk: 8,
                mosi: 7,
                miso: 6,
                khz: 100,
                mode: 0,
            }]
        );
        assert_eq!(
            hw.spis,
            [SpiSpec {
                name: "tester".into(),
                bus: "spi2".into(),
                cs: 9,
            }]
        );
    }

    #[test]
    fn spi_defaults_are_1_mhz_mode_0_and_explicit_attrs_are_parsed() {
        let src = WITH_SPI.replace(" frequency=\"100kHz\"", " mode=\"3\"");
        let bus = &lower_src(&src).unwrap().spi_buses[0];
        assert_eq!(bus.khz, 1000);
        assert_eq!(bus.mode, 3);

        let bad = WITH_SPI.replace("frequency=\"100kHz\"", "mode=\"4\"");
        assert!(lower_src(&bad).unwrap_err().contains("must be 0, 1, 2 or 3"));

        let mhz = WITH_SPI.replace("100kHz", "105MHZ");
        assert!(lower_src(&mhz).unwrap_err().contains("not supported yet"));
    }

    #[test]
    fn a_spi_component_needs_a_declared_spi() {
        // The ref passes graph resolution (`spi2` becomes a gpio peripheral)
        // but the component then points at a gpio, not an `<spi>`. Step 2c:
        // the `$` lookup goes through `SpiComponent::resolve`, so the
        // wrapper's message replaces hardware's old "not an <spi>" line.
        let src = WITH_SPI.replace(
            "<spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"100kHz\" />",
            "<gpio id=\"spi2\" pin=\"8\" direction=\"output\" />",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("expects an spi"), "{err}");
    }

    #[test]
    fn spi_ids_must_be_spi2() {
        let src = WITH_SPI
            .replace("<spi id=\"spi2\"", "<spi id=\"bus0\"")
            .replace("spi=\"$spi2\"", "spi=\"$bus0\"");
        assert!(lower_src(&src).unwrap_err().contains("the id must be spi2"));
    }

    #[test]
    fn a_bare_spi_without_a_component_generates_nothing() {
        let src = WITH_SPI.replace(
            "# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />\n",
            "# components\n",
        );
        let hw = lower_src(&src).unwrap();
        assert!(hw.spis.is_empty());
        assert!(hw.spi_buses.is_empty());
    }

    #[test]
    fn several_spi_devices_may_share_one_bus_but_not_one_cs() {
        let src = WITH_SPI.replace(
            "# devices",
            "  <spi_component id=\"second\" spi=\"$spi2\" cs=\"10\" />\n# devices",
        );
        let hw = lower_src(&src).unwrap();
        assert_eq!(hw.spis.len(), 2);
        assert_eq!(hw.spi_buses.len(), 1, "bus is emitted once");

        let same_cs = WITH_SPI.replace(
            "# devices",
            "  <spi_component id=\"second\" spi=\"$spi2\" cs=\"9\" />\n# devices",
        );
        let err = lower_src(&same_cs).unwrap_err();
        assert!(
            err.contains("tester") && err.contains("second") && err.contains("both use GPIO9"),
            "{err}"
        );
    }

    #[test]
    fn a_spi_component_needs_a_cs_pin() {
        // Step 2c: missing `cs` is now reported by `SpiComponent::resolve`.
        let src = WITH_SPI.replace(" cs=\"9\"", "");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("missing required attribute 'cs'"), "{err}");
    }

    #[test]
    fn spi_pins_and_cs_must_not_clash_with_anything_else() {
        let with_led = WITH_SPI.replace(
            "# peripherals",
            "# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />",
        );
        let with_led = with_led.replace(
            "# components",
            "# components\n  <led id=\"red\" gpio=\"$gpio9\" />",
        );
        let err = lower_src(&with_led).unwrap_err();
        assert!(
            err.contains("gpio9") && err.contains("tester") && err.contains("GPIO9"),
            "{err}"
        );

        // a bus pin colliding with an I2C pin
        let both = WITH_BUS
            .replace("sda=\"4\" scl=\"5\"", "sda=\"8\" scl=\"5\"")
            .replace(
                "# peripherals",
                "# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" />",
            )
            .replace(
                "# components",
                "# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />",
            );
        let err = lower_src(&both).unwrap_err();
        assert!(
            err.contains("<i2c id=\"i2c0\">") && err.contains("<spi id=\"spi2\">"),
            "{err}"
        );

        // a device's cs on its own bus pin would hold CS low permanently
        let cs_on_bus = WITH_SPI
            .replace(" miso=\"6\"", " miso=\"9\"")
            .replace("cs=\"9\"", "cs=\"9\"");
        let err = lower_src(&cs_on_bus).unwrap_err();
        assert!(err.contains("both use GPIO9"), "{err}");
    }
}

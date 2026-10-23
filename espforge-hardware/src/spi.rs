//! `<spi>` peripheral + `<spi_component>` lowering (step 1d).
//!
//! Moved verbatim from `espforge/src/commands/pipeline.rs::hardware`: the
//! `<spi>` peripheral branch (`spi2`-only rule, mode, frequency) and the
//! `<spi_component>` component branch (bus ref, `cs` pin, one-CS-per-pin).

use super::util::parse_khz;
use super::{SpiBusSpec, SpiSpec, is_rust_ident};
use espf::context::ParsedNode;

/// One `<spi>` peripheral (`spi2`), before filtering bare buses.
#[derive(Debug, Clone)]
pub struct SpiBusDraft {
    /// The `<spi>` id (`spi2`), which is also the peripheral name.
    pub id: String,
    pub sclk: u8,
    pub mosi: u8,
    pub miso: u8,
    pub khz: u32,
    pub mode: u8,
    /// Whether any `<spi_component>` rides on this bus.
    pub used: bool,
}

/// One `<spi_component>`: a device with its own chip-select.
#[derive(Debug, Clone)]
pub struct SpiDeviceDraft {
    /// The `<spi_component>` id as a Rust identifier.
    pub name: String,
    /// The bus id it rides on.
    pub bus: String,
    pub cs: u8,
}

/// The `<spi>` branch of the `# peripherals` loop.
pub fn lower_spi(n: &ParsedNode) -> Result<SpiBusDraft, String> {
    // The runtime `SpiBus::build` is typed on `SPI2`; every chip
    // espforge supports exposes it (GPIO matrix: any pin works).
    if n.id != "spi2" {
        return Err(format!("<spi id=\"{}\">: the id must be spi2", n.id));
    }
    let pin_attr = |what: &str| {
        n.get_attr(what)
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| format!("<spi id=\"{}\"> needs a numeric {what}=\"...\"", n.id))
    };
    let mode = match n.get_attr("mode") {
        None => 0,
        Some("0") => 0,
        Some("1") => 1,
        Some("2") => 2,
        Some("3") => 3,
        Some(other) => {
            return Err(format!(
                "<spi id=\"{}\"> mode=\"{other}\" must be 0, 1, 2 or 3",
                n.id
            ));
        }
    };
    let khz = match n.get_attr("frequency") {
        // The `SpiConfig` default in espforge-runtime (1 MHz).
        None => 1000,
        Some(f) => parse_khz(f, "spi", n.id)?,
    };
    Ok(SpiBusDraft {
        id: n.id.to_string(),
        sclk: pin_attr("sclk")?,
        mosi: pin_attr("mosi")?,
        miso: pin_attr("miso")?,
        khz,
        mode,
        used: false,
    })
}

/// The `<spi_component>` branch of the `# components` loop. Pushes into
/// `devices` and marks the bus used.
pub fn attach_spi_component(
    n: &ParsedNode,
    spi_buses: &mut [SpiBusDraft],
    spi_devices: &mut Vec<SpiDeviceDraft>,
) -> Result<(), String> {
    let bus_ref = n.get_attr("spi").ok_or_else(|| {
        format!("<spi_component id=\"{}\"> needs spi=\"$<spi id>\"", n.id)
    })?;
    let target = bus_ref.trim_start_matches('$');
    if !spi_buses.iter().any(|b| b.id == target) {
        return Err(format!(
            "<spi_component id=\"{}\"> refers to `{target}`, which is not an <spi> in `# peripherals`",
            n.id
        ));
    }
    let cs = n
        .get_attr("cs")
        .and_then(|p| p.parse::<u8>().ok())
        .ok_or_else(|| {
            format!("<spi_component id=\"{}\"> needs a numeric cs=\"...\"", n.id)
        })?;
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(format!(
            "<spi_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
            n.id
        ));
    }
    if let Some(other) = spi_devices.iter().find(|d| d.cs == cs) {
        return Err(format!(
            "<spi_component id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{cs}",
            other.name, name
        ));
    }
    spi_buses
        .iter_mut()
        .find(|b| b.id == target)
        .expect("checked above")
        .used = true;
    spi_devices.push(SpiDeviceDraft {
        name,
        bus: target.to_string(),
        cs,
    });
    Ok(())
}

pub fn bus_pins(b: &SpiBusDraft) -> [u8; 3] {
    [b.sclk, b.mosi, b.miso]
}

/// A bare `<spi>` with no component generates nothing: the app can only
/// reach the bus through a component anyway.
pub fn finish_spi(
    buses: Vec<SpiBusDraft>,
    devices: Vec<SpiDeviceDraft>,
) -> (Vec<SpiBusSpec>, Vec<SpiSpec>) {
    let spis: Vec<SpiSpec> = devices
        .iter()
        .map(|d| SpiSpec {
            name: d.name.clone(),
            bus: d.bus.clone(),
            cs: d.cs,
        })
        .collect();
    let bus_specs: Vec<SpiBusSpec> = buses
        .into_iter()
        .filter(|b| b.used)
        .map(|b| SpiBusSpec {
            id: b.id.clone(),
            peripheral: "SPI2".to_string(),
            sclk: b.sclk,
            mosi: b.mosi,
            miso: b.miso,
            khz: b.khz,
            mode: b.mode,
        })
        .collect();
    (bus_specs, spis)
}

#[cfg(test)]
mod tests {
    use super::super::lower_sections;
    use super::*;

    const WITH_SPI: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"100kHz\" />\n# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />\n# devices\n# services\n";

    const WITH_BUS: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<super::super::Hardware, String> {
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
        // but the component then points at a gpio, not an `<spi>`.
        let src = WITH_SPI.replace(
            "<spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"100kHz\" />",
            "<gpio id=\"spi2\" pin=\"8\" direction=\"output\" />",
        );
        assert!(lower_src(&src).unwrap_err().contains("not an <spi>"));
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
        let src = WITH_SPI.replace(" cs=\"9\"", "");
        assert!(lower_src(&src).unwrap_err().contains("cs=\"...\""));
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

//! `<spi>` peripheral lowering.
//!
//! The `<spi_component>` branch lives in
//! [`crate::components::spi_component`]; the bus/device drafts below are
//! shared between the two.

use crate::util::parse_khz;
use crate::{SpiBusSpec, SpiSpec};
use espf::context::ParsedNode;
use espf::diag::EspfError;

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
pub fn lower_spi(n: &ParsedNode) -> Result<SpiBusDraft, EspfError> {
    // The runtime `SpiBus::build` is typed on `SPI2`; every chip
    // espforge supports exposes it (GPIO matrix: any pin works).
    if n.id != "spi2" {
        return Err(EspfError::at_node(
            n,
            "invalid spi id",
            format!("<spi id=\"{}\">: the id must be spi2", n.id),
        ));
    }
    let pin_attr = |what: &str| -> Result<u8, EspfError> {
        n.get_attr(what).and_then(|p| p.parse::<u8>().ok()).ok_or_else(|| {
            EspfError::at_node(
                n,
                "invalid spi pin",
                format!("<spi id=\"{}\"> needs a numeric {what}=\"...\"", n.id),
            )
        })
    };
    let mode = match n.get_attr("mode") {
        None => 0,
        Some("0") => 0,
        Some("1") => 1,
        Some("2") => 2,
        Some("3") => 3,
        Some(other) => {
            return Err(EspfError::at_node(
                n,
                "invalid spi mode",
                format!(
                    "<spi id=\"{}\"> mode=\"{other}\" must be 0, 1, 2 or 3",
                    n.id
                ),
            ));
        }
    };
    let khz = match n.get_attr("frequency") {
        // The `SpiConfig` default in espforge-runtime (1 MHz).
        None => 1000,
        Some(f) => parse_khz(f, "spi", n.id)
            .map_err(|m| EspfError::at_node(n, "invalid spi frequency", m))?,
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

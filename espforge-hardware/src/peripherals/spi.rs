//! `<spi>` peripheral lowering.
//!
//! The `<spi_component>` branch lives in
//! [`crate::components::spi_component`]; the bus/device drafts below are
//! shared between the two.

use crate::util::parse_khz;
use crate::{SpiBusSpec, SpiSpec};
use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_bindings::components::attrs::Issue;
use espforge_bindings::components::generated_elements::spi as schema;

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
    let attrs = schema::Attrs::from_node(n).map_err(|issue| {
        let (title, message) = describe(n.id, issue);
        EspfError::at_node(n, title, message)
    })?;
    // `mode` is schema-checked (0..=3, default 0); hardware turns it into u8.
    let mode = attrs.mode.value as u8;
    // The schema lets `miso` be omitted (displays do); this runtime does not,
    // so the requirement and its message stay here until the diagram side
    // decides what an absent MISO means.
    let miso = attrs.miso.map(|miso| miso.value).ok_or_else(|| {
        EspfError::at_node(
            n,
            "invalid spi pin",
            format!("<spi id=\"{}\"> needs a numeric miso=\"...\"", n.id),
        )
    })?;
    let khz = match attrs.frequency {
        // The `SpiConfig` default in espforge-runtime (1 MHz).
        None => 1000,
        Some(f) => parse_khz(f.value, "spi", n.id)
            .map_err(|m| EspfError::at_node(n, "invalid spi frequency", m))?,
    };
    Ok(SpiBusDraft {
        id: n.id.to_string(),
        sclk: attrs.sclk.value,
        mosi: attrs.mosi.value,
        miso,
        khz,
        mode,
        used: false,
    })
}

/// This element's wording for a schema-level problem: `mode` keeps its own
/// title, everything else is a pin problem.
fn describe(id: &str, issue: Issue<'_>) -> (&'static str, String) {
    match issue {
        Issue::Invalid {
            attr: "mode",
            value,
        } => (
            "invalid spi mode",
            format!("<spi id=\"{id}\"> mode=\"{value}\" must be 0, 1, 2 or 3"),
        ),
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => (
            "invalid spi pin",
            format!("<spi id=\"{id}\"> needs a numeric {attr}=\"...\""),
        ),
    }
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

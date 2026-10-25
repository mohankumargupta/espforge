//! `<spi>` peripheral lowering.
//!
//! The `<spi_component>` branch lives in [`crate::board`]; this module only
//! turns one `<spi>` node into its pins, frequency and mode.

use crate::util::parse_khz;
use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_bindings::components::attrs::Issue;
use espforge_bindings::components::generated_elements::spi as schema;

/// One `<spi>` peripheral (`spi2`).
#[derive(Debug, Clone)]
pub struct SpiPeripheral {
    /// The `<spi>` id (`spi2`), which is also the peripheral name.
    pub id: String,
    pub sclk: u8,
    pub mosi: u8,
    /// `None` is a write-only bus: the schema lets `miso` be omitted, and a
    /// display never reads from the bus anyway.
    pub miso: Option<u8>,
    pub khz: u32,
    pub mode: u8,
}

/// The `<spi>` branch of the `# peripherals` loop.
pub fn lower_spi(n: &ParsedNode) -> Result<SpiPeripheral, EspfError> {
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
    // `miso` is optional in the schema (displays omit it) and optional here:
    // an absent one means the generated bus is write-only.
    let miso = attrs.miso.map(|miso| miso.value);
    let khz = match attrs.frequency {
        // The `SpiConfig` default in espforge-runtime (1 MHz).
        None => 1000,
        Some(f) => parse_khz(f.value, "spi", n.id)
            .map_err(|m| EspfError::at_node(n, "invalid spi frequency", m))?,
    };
    Ok(SpiPeripheral {
        id: n.id.to_string(),
        sclk: attrs.sclk.value,
        mosi: attrs.mosi.value,
        miso,
        khz,
        mode,
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

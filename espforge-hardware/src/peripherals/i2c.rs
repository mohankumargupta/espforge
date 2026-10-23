//! `<i2c>` peripheral lowering.
//!
//! The `<i2c_component>` branch lives in
//! [`crate::components::i2c_component`]; the bus draft below is shared
//! between the two.

use crate::util::parse_khz;
use crate::I2cSpec;
use espf::context::ParsedNode;
use espf::diag::EspfError;

/// One `<i2c>` peripheral plus the component riding on it (if any).
#[derive(Debug, Clone)]
pub struct I2cBusDraft {
    pub id: String,
    pub peripheral: String,
    pub sda: u8,
    pub scl: u8,
    pub khz: u32,
    /// The component id riding on this bus, if any.
    pub used: Option<String>,
}

/// `i2c0` -> `I2C0`, `i2c1` -> `I2C1`, `i2c2` -> `I2C2`; anything else is the
/// user's error (the esp-hal peripheral structs are named exactly this).
fn i2c_peripheral_name(n: &ParsedNode) -> Result<String, EspfError> {
    let id = n.id;
    if id == "i2c0" || id == "i2c1" || id == "i2c2" {
        return Ok(id.to_ascii_uppercase());
    }
    Err(EspfError::at_node(
        n,
        "invalid i2c id",
        format!("<i2c id=\"{id}\">: the id must be i2c0, i2c1 or i2c2"),
    ))
}

/// The `<i2c>` branch of the `# peripherals` loop.
pub fn lower_i2c(n: &ParsedNode) -> Result<I2cBusDraft, EspfError> {
    let pin_attr = |what: &str| -> Result<u8, EspfError> {
        n.get_attr(what)
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| {
                EspfError::at_node(
                    n,
                    "invalid i2c pin",
                    format!("<i2c id=\"{}\"> needs a numeric {what}=\"...\"", n.id),
                )
            })
    };
    let khz = match n.get_attr("frequency") {
        None => 100, // esp-hal's default
        Some(f) => parse_khz(f, "i2c", n.id)
            .map_err(|m| EspfError::at_node(n, "invalid i2c frequency", m))?,
    };
    Ok(I2cBusDraft {
        id: n.id.to_string(),
        peripheral: i2c_peripheral_name(n)?,
        sda: pin_attr("sda")?,
        scl: pin_attr("scl")?,
        khz,
        used: None,
    })
}

/// A bare `<i2c>` with no component generates nothing: the app can only
/// reach the bus through a component anyway.
pub fn finish_i2c(buses: Vec<I2cBusDraft>) -> Vec<I2cSpec> {
    buses
        .into_iter()
        .filter_map(|b| {
            b.used.map(|name| I2cSpec {
                name,
                peripheral: b.peripheral,
                sda: b.sda,
                scl: b.scl,
                khz: b.khz,
            })
        })
        .collect()
}

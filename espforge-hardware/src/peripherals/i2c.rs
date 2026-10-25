//! `<i2c>` peripheral lowering.
//!
//! The `<i2c_component>` branch lives in [`crate::board`]; this module only
//! turns one `<i2c>` node into its pins, frequency and peripheral name.

use crate::util::parse_khz;
use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_bindings::components::attrs::Issue;
use espforge_bindings::components::generated_elements::i2c as schema;

/// One `<i2c>` peripheral.
#[derive(Debug, Clone)]
pub struct I2cPeripheral {
    pub id: String,
    pub peripheral: String,
    pub sda: u8,
    pub scl: u8,
    pub khz: u32,
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
pub fn lower_i2c(n: &ParsedNode) -> Result<I2cPeripheral, EspfError> {
    let attrs = schema::Attrs::from_node(n)
        .map_err(|issue| EspfError::at_node(n, "invalid i2c pin", message(n.id, issue)))?;
    // The schema's default is `100kHz`, which is esp-hal's default as well.
    let khz = parse_khz(attrs.frequency.value, "i2c", n.id)
        .map_err(|m| EspfError::at_node(n, "invalid i2c frequency", m))?;
    Ok(I2cPeripheral {
        id: n.id.to_string(),
        peripheral: i2c_peripheral_name(n)?,
        sda: attrs.sda.value,
        scl: attrs.scl.value,
        khz,
    })
}

/// This element's wording for a schema-level problem.
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => {
            format!("<i2c id=\"{id}\"> needs a numeric {attr}=\"...\"")
        }
    }
}

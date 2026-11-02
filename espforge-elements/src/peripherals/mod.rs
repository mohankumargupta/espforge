//! Elements that live in `# peripherals`: hardware the chip provides.

pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;
pub mod wifi;

/// A `frequency=` value like `100kHz` or `400kHz` -> kHz; anything else is
/// told off. `tag` is the element name used in the diagnostics (`i2c`, `spi`).
pub(crate) fn parse_khz(raw: &str, tag: &str, id: &str) -> Result<u32, String> {
    let t = raw.trim().to_ascii_lowercase();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: u32 = num.parse().map_err(|_| {
        format!("<{tag} id=\"{id}\"> frequency=\"{raw}\" is not a frequency: use e.g. `100kHz`")
    })?;
    match unit.trim() {
        "khz" => Ok(n),
        other => Err(format!(
            "<{tag} id=\"{id}\"> frequency=\"{raw}\" unit `{other}` is not supported yet: use `kHz`"
        )),
    }
}

//! Shared hardware helpers (step 1d).
//!
//! `parse_khz` is used by both `i2c` (`frequency="400kHz"`) and `spi`
//! (`frequency="100kHz"`). It is kept here verbatim from
//! `espforge/src/commands/pipeline.rs` so both buses share one copy instead
//! of duplicating the 20 lines per bus.

use espf::diag::EspfError;

/// A `frequency=` value like `100kHz` or `400kHz` -> kHz; anything else is
/// told off. `tag` is the element name used in the diagnostics (`i2c`, `spi`).
pub fn parse_khz(raw: &str, tag: &str, id: &str) -> Result<u32, String> {
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

/// An `EspfError` as a one-line message (the hardware step reports plain
/// strings, like the rest of `build`'s "not supported yet" messages).
pub fn plain(e: EspfError) -> String {
    format!("{}: {}", e.title, e.label)
}

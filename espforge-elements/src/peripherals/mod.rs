//! Elements that live in `# peripherals`: hardware the chip provides.

pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;
pub mod wifi;

/// A `frequency=` value like `100kHz`, `1MHz` or `400000Hz` -> kHz; anything
/// else is told off. `tag` is the element name used in the diagnostics (`i2c`,
/// `spi`).
///
/// The three spellings match `driver.rs`'s documentation: a manifest may write
/// `frequency "40000kHz"` or `"40MHz"` and both mean the same clock.
pub(crate) fn parse_khz(raw: &str, tag: &str, id: &str) -> Result<u32, String> {
    let t = raw.trim().to_ascii_lowercase();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: u32 = num.parse().map_err(|_| {
        format!("<{tag} id=\"{id}\"> frequency=\"{raw}\" is not a frequency: use e.g. `100kHz`")
    })?;
    match unit.trim() {
        "khz" => Ok(n),
        "mhz" => Ok(n.saturating_mul(1_000)),
        "hz" => {
            if n == 0 || n % 1_000 != 0 {
                return Err(format!(
                    "<{tag} id=\"{id}\"> frequency=\"{raw}\" is not a whole number of kHz: use \
                     a multiple of 1000Hz, or `kHz`/`MHz`"
                ));
            }
            Ok(n / 1_000)
        }
        other => Err(format!(
            "<{tag} id=\"{id}\"> frequency=\"{raw}\" unit `{other}` is not supported: use `kHz`, `MHz` or `Hz`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_spellings_are_the_same_clock() {
        assert_eq!(parse_khz("100kHz", "spi", "spi2"), Ok(100));
        assert_eq!(parse_khz("40MHz", "spi", "spi2"), Ok(40_000));
        assert_eq!(parse_khz("40000kHz", "spi", "spi2"), Ok(40_000));
        assert_eq!(parse_khz("400000Hz", "spi", "spi2"), Ok(400));
    }

    #[test]
    fn hz_must_be_a_whole_number_of_khz() {
        assert!(parse_khz("0Hz", "spi", "spi2").is_err());
        assert!(parse_khz("1500Hz", "spi", "spi2").is_err());
        assert!(parse_khz("100miles", "spi", "spi2").is_err());
    }
}

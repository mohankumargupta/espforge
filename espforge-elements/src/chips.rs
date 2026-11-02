//! Chip facts used by validation.

/// Chips `esp-metadata-generated` knows about that espforge has no template
/// or Wokwi board for yet. Remove an entry once it is supported.
pub const NOT_YET_SUPPORTED: [&str; 1] = ["esp32p4"];

/// The metadata entry for a chip name, if the catalog knows it.
fn chip_config(name: &str) -> Option<esp_metadata_generated::Chip> {
    esp_metadata_generated::Chip::iter().find(|c| c.name() == name)
}

/// Every chip `esp-metadata-generated` knows, by name.
pub fn supported_chips() -> Vec<&'static str> {
    esp_metadata_generated::Chip::iter()
        .map(|c| c.name())
        .collect()
}

/// The chips to offer in prompts: everything known except
/// [`NOT_YET_SUPPORTED`].
pub fn offered_chips() -> Vec<&'static str> {
    supported_chips()
        .into_iter()
        .filter(|c| !NOT_YET_SUPPORTED.contains(c))
        .collect()
}

pub fn is_supported_chip(name: &str) -> bool {
    chip_config(name).is_some()
}

/// Whether the chip has the Wi-Fi radio block.
///
/// The answer is a symbol in `esp-metadata`'s per-chip data (`soc_has_wifi`),
/// so a new chip is right without editing this file — which is why `<wifi>`
/// has no `compatible` chip list. A name the catalog does not know has no
/// radio rather than panicking: the chip name itself is validated elsewhere.
pub fn has_wifi(name: &str) -> bool {
    chip_config(name).is_some_and(|chip| chip.contains("soc_has_wifi"))
}

/// Whether the chip has the built-in USB-Serial-JTAG peripheral.
///
/// NOTE: this is a hand-kept list (it replaces the old `chip == "esp32"`
/// special case). It should eventually be read from the esp-metadata
/// peripheral data instead; this function is the only place to change.
pub fn has_usb_serial_jtag(chip: &str) -> bool {
    matches!(
        chip,
        "esp32c3" | "esp32c5" | "esp32c6" | "esp32h2" | "esp32s3" | "esp32p4"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate `<wifi>` lowers through. h2 and p4 are the two chips
    /// `esp-metadata` reports without a radio, and they are the reason there is
    /// no `compatible` chip list in the schema.
    #[test]
    fn the_radio_follows_the_chip() {
        assert!(has_wifi("esp32c3"));
        assert!(has_wifi("esp32s3"));
        assert!(has_wifi("esp32"));
        assert!(!has_wifi("esp32h2"));
        assert!(!has_wifi("esp32p4"));
    }

    #[test]
    fn an_unknown_chip_has_no_radio() {
        assert!(!has_wifi("nonesuch"));
        assert!(!is_supported_chip("nonesuch"));
        assert!(is_supported_chip("esp32c3"));
    }
}

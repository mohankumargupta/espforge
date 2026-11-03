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

/// Why `name` cannot be the target of a project, or nothing when it can.
///
/// `--chip` and the `new` prompt both ask here, so a chip that
/// `esp-metadata-generated` knows but espforge has no template or Wokwi board
/// for ([`NOT_YET_SUPPORTED`]) is refused with that reason instead of being
/// accepted and discovered much later, by `build`.
pub fn check_chip(name: &str) -> Result<(), String> {
    if NOT_YET_SUPPORTED.contains(&name) {
        return Err(format!(
            "chip \"{name}\" is known to esp-metadata but not supported by espforge yet (expected one of: {})",
            offered_chips().join(", ")
        ));
    }
    if !is_supported_chip(name) {
        return Err(format!(
            "unknown chip \"{name}\" for --chip (expected one of: {})",
            supported_chips().join(", ")
        ));
    }
    Ok(())
}

/// A one-line summary of a chip, for prompts; `None` for a chip espforge has
/// no blurb for yet.
///
/// NOTE: hand-kept, like [`has_usb_serial_jtag`]. This function is the only
/// place to change, and `every_offered_chip_has_a_description` keeps it in
/// step with the chips actually offered.
pub fn description(name: &str) -> Option<&'static str> {
    Some(match name {
        "esp32" => "Xtensa dual-core, Wi-Fi + Bluetooth Classic/BLE",
        "esp32c2" => "RISC-V single-core, Wi-Fi 4 + BLE 5 (entry-level)",
        "esp32c3" => "RISC-V single-core, Wi-Fi 4 + BLE 5",
        "esp32c5" => "RISC-V single-core, dual-band Wi-Fi 6 + BLE 5 + 802.15.4",
        "esp32c6" => "RISC-V single-core, Wi-Fi 6 + BLE 5 + 802.15.4",
        "esp32c61" => "RISC-V single-core, Wi-Fi 6 + BLE 5 + 802.15.4",
        "esp32h2" => "RISC-V single-core, BLE 5 + 802.15.4 (no Wi-Fi)",
        "esp32s2" => "Xtensa single-core, Wi-Fi 4, USB-OTG",
        "esp32s3" => "Xtensa dual-core, Wi-Fi 4 + BLE 5, USB-OTG",
        "esp32s31" => "RISC-V dual-core, Wi-Fi 6 + BLE 5.4 + 802.15.4 + Ethernet",
        _ => return None,
    })
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

    #[test]
    fn an_unknown_chip_is_refused_by_name() {
        let err = check_chip("nonesuch").unwrap_err();
        assert!(err.contains("unknown chip \"nonesuch\""), "{err}");
        assert!(check_chip("esp32c3").is_ok());
    }

    #[test]
    fn a_recognised_chip_espforge_cannot_build_is_refused_for_that_reason() {
        let err = check_chip(NOT_YET_SUPPORTED[0]).unwrap_err();
        assert!(err.contains("not supported by espforge yet"), "{err}");
        assert!(!err.contains("unknown chip"), "{err}");
    }

    #[test]
    fn every_offered_chip_has_a_description() {
        let missing: Vec<&str> = offered_chips()
            .into_iter()
            .filter(|chip| description(chip).is_none())
            .collect();
        assert!(missing.is_empty(), "no description for {missing:?}");
    }
}

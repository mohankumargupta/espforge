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

/// Whether the chip has an RMT (remote control transceiver) block.
///
/// Read from `esp-metadata`'s per-chip data (`soc_has_rmt`), like
/// [`has_wifi`]: a name the catalog does not know has no RMT rather than
/// panicking, because the chip name itself is validated elsewhere.
pub fn has_rmt(name: &str) -> bool {
    chip_config(name).is_some_and(|chip| chip.contains("soc_has_rmt"))
}

/// The RMT channels a chip exposes, and the clock their pulse lengths are
/// counted against.
///
/// `tx_channels` and `rx_channels` are the **channel numbers** `esp-hal`
/// exposes, not indices into a per-direction pool. On `esp32` and `esp32s2`
/// the same numbers appear in both lists because every channel can do either
/// direction; on the others the two lists are disjoint. A caller must claim
/// the number itself, never `(number, direction)`.
pub struct RmtCaps {
    pub tx_channels: &'static [u8],
    pub rx_channels: &'static [u8],
    /// The RMT source clock in MHz: 80 on every supported chip but `esp32h2`.
    pub base_mhz: u32,
}

const CH_0_1: &[u8] = &[0, 1];
const CH_2_3: &[u8] = &[2, 3];
const CH_0_3: &[u8] = &[0, 1, 2, 3];
const CH_4_7: &[u8] = &[4, 5, 6, 7];
const CH_0_7: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];

/// The RMT table for `chip`, or `None` when the chip has no verified RMT.
///
/// Every chip the catalog knows is listed explicitly, including the ones that
/// deliberately have no answer: a new chip must be added here, with its
/// channels checked against `esp-metadata`, instead of silently inheriting
/// another chip's table. `esp32c5`, `esp32s31` and `esp32p4` advertise RMT in
/// `esp-metadata` but lose it until their channel map is verified — a wrong
/// table compiles and fails on hardware.
pub fn rmt_caps(chip: &str) -> Option<RmtCaps> {
    let caps = match chip {
        "esp32" => (CH_0_7, CH_0_7, 80),
        "esp32s2" => (CH_0_3, CH_0_3, 80),
        "esp32s3" => (CH_0_3, CH_4_7, 80),
        "esp32c3" | "esp32c6" => (CH_0_1, CH_2_3, 80),
        // ESP32-H2's default RMT source is XTAL, a 32MHz crystal.
        "esp32h2" => (CH_0_1, CH_2_3, 32),
        "esp32c2" | "esp32c61" => return None,
        "esp32c5" | "esp32s31" | "esp32p4" => return None,
        _ => return None,
    };
    Some(RmtCaps {
        tx_channels: caps.0,
        rx_channels: caps.1,
        base_mhz: caps.2,
    })
}

/// How many I2C controllers the chip has.
///
/// Hand-kept, like [`has_usb_serial_jtag`]: esp-metadata's generated data does
/// not name a controller count. The RISC-V entry-level parts expose one; every
/// other supported chip exposes two.
pub fn i2c_instances(chip: &str) -> usize {
    match chip {
        "esp32c2" | "esp32c3" | "esp32c6" | "esp32h2" => 1,
        _ => 2,
    }
}

/// How many UART ports the chip has.
pub fn uart_instances(chip: &str) -> usize {
    match chip {
        "esp32c2" | "esp32c3" | "esp32c6" | "esp32h2" => 2,
        _ => 3,
    }
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

    /// The controller counts the peripheral rules read: a single-I2C chip
    /// cannot be asked for `i2c1`, and a two-UART chip has no `UART2`.
    #[test]
    fn the_controller_counts_follow_the_chip() {
        assert_eq!(i2c_instances("esp32c3"), 1);
        assert_eq!(i2c_instances("esp32c6"), 1);
        assert_eq!(i2c_instances("esp32s3"), 2);
        assert_eq!(uart_instances("esp32c3"), 2);
        assert_eq!(uart_instances("esp32s3"), 3);
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
    fn the_rmt_gate_follows_the_chip() {
        assert!(!has_rmt("esp32c2"));
        assert!(!has_rmt("esp32c61"));
        assert!(has_rmt("esp32c3"));
        assert!(has_rmt("esp32s3"));
        assert!(has_rmt("esp32"));
        assert!(!has_rmt("nonesuch"));
    }

    /// `esp32h2` counts pulse lengths against a 32MHz crystal, not the 80MHz
    /// APB clock every other RMT-capable chip uses.
    #[test]
    fn the_rmt_source_clock_follows_the_chip() {
        assert_eq!(rmt_caps("esp32h2").unwrap().base_mhz, 32);
        assert_eq!(rmt_caps("esp32c3").unwrap().base_mhz, 80);
        assert_eq!(rmt_caps("esp32s3").unwrap().base_mhz, 80);
    }

    /// The TX/RX pools overlap exactly where `esp-hal` says they do: esp32 and
    /// esp32s2 let any channel do either direction.
    #[test]
    fn the_shared_channel_chips_are_recorded_as_shared() {
        for chip in ["esp32", "esp32s2"] {
            let caps = rmt_caps(chip).unwrap();
            assert_eq!(caps.tx_channels, caps.rx_channels, "{chip}");
        }
        let c3 = rmt_caps("esp32c3").unwrap();
        assert!(
            c3.tx_channels.iter().all(|tx| !c3.rx_channels.contains(tx)),
            "esp32c3's TX and RX pools are disjoint"
        );
    }

    #[test]
    fn every_rmt_answer_is_well_formed() {
        for chip in offered_chips() {
            match rmt_caps(chip) {
                Some(caps) => {
                    assert!(has_rmt(chip), "{chip} has caps but no RMT");
                    for channel in caps.tx_channels.iter().chain(caps.rx_channels) {
                        assert!(*channel < 8, "{chip} names channel {channel}");
                    }
                }
                None => assert!(
                    matches!(
                        chip,
                        "esp32c2" | "esp32c5" | "esp32c61" | "esp32p4" | "esp32s31"
                    ),
                    "{chip} has neither RMT caps nor a deliberate `None`"
                ),
            }
        }
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

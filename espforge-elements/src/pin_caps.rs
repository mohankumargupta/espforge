//! Per-chip pin capabilities, consulted when a pin is claimed.
//!
//! Generated from `esp-metadata-generated` 0.5.3 (the per-chip `pins` table it
//! is itself generated from), then committed: `esp-metadata` is a build-script
//! dependency and this check belongs in lowering, which runs on the host.
//!
//! A chip this table does not name is a hard error at generation
//! ([`espforge_diagnostics::codes::PIN_CAPS_MISSING`]), never a silent skip;
//! `every_offered_chip_has_a_table` fails when an offered chip loses its table.

/// One chip's pin facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinCaps {
    /// How many pins the chip has: valid pin numbers are `0 .. pins`.
    pub pins: u8,
    /// Pins with no output driver: usable as inputs only.
    pub input_only: &'static [u8],
    /// Pins wired to the SPI flash or PSRAM bus. Never usable by application
    /// code: driving one corrupts the bus the CPU executes from.
    pub flash: &'static [u8],
    /// Pins whose level at reset selects a boot mode. Usable, but a warning,
    /// because an external driver can change how the chip boots.
    pub strapping: &'static [u8],
}

/// The pin table for `chip`, if espforge has one.
pub fn caps(chip: &str) -> Option<&'static PinCaps> {
    CAPS.iter()
        .find(|(name, _)| *name == chip)
        .map(|(_, caps)| caps)
}

/// Every chip this table was generated for.
static CAPS: &[(&str, PinCaps)] = &[
    (
        "esp32",
        PinCaps {
            pins: 40,
            input_only: &[34, 35, 36, 37, 38, 39],
            flash: &[6, 7, 8, 9, 10, 11, 16, 17],
            strapping: &[0, 2, 5, 12, 15],
        },
    ),
    (
        "esp32c2",
        PinCaps {
            pins: 21,
            input_only: &[],
            flash: &[11, 12, 13, 14, 15, 16, 17],
            strapping: &[8, 9],
        },
    ),
    (
        "esp32c3",
        PinCaps {
            pins: 22,
            input_only: &[],
            flash: &[11, 12, 13, 14, 15, 16, 17],
            strapping: &[2, 8, 9],
        },
    ),
    (
        "esp32c5",
        PinCaps {
            pins: 29,
            input_only: &[],
            flash: &[15, 16, 17, 18, 19, 20, 21, 22],
            strapping: &[2, 3, 7, 25, 26, 27, 28],
        },
    ),
    (
        "esp32c6",
        PinCaps {
            pins: 31,
            input_only: &[],
            flash: &[24, 25, 26, 27, 28, 29, 30],
            strapping: &[4, 5, 8, 9, 15],
        },
    ),
    (
        "esp32c61",
        PinCaps {
            pins: 30,
            input_only: &[],
            flash: &[14, 15, 16, 17, 18, 19, 20, 21],
            strapping: &[3, 4, 7, 8, 9],
        },
    ),
    (
        "esp32h2",
        PinCaps {
            pins: 28,
            input_only: &[],
            flash: &[6, 7],
            strapping: &[8, 9, 25],
        },
    ),
    (
        "esp32p4",
        PinCaps {
            pins: 55,
            input_only: &[],
            flash: &[],
            strapping: &[32, 33, 34, 35, 36, 37, 38],
        },
    ),
    (
        "esp32s2",
        PinCaps {
            pins: 47,
            input_only: &[],
            flash: &[26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37],
            strapping: &[0, 45, 46],
        },
    ),
    (
        "esp32s3",
        PinCaps {
            pins: 49,
            input_only: &[],
            flash: &[26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37],
            strapping: &[0, 3, 45, 46],
        },
    ),
    (
        "esp32s31",
        PinCaps {
            pins: 62,
            input_only: &[],
            flash: &[26, 27, 28, 30, 31, 32],
            strapping: &[27, 28, 36, 37, 38, 39, 40, 60, 61],
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every chip espforge can target has a table, so a pin check is never
    /// silently skipped.
    #[test]
    fn every_offered_chip_has_a_table() {
        for chip in crate::chips::offered_chips() {
            assert!(caps(chip).is_some(), "no pin table for `{chip}`");
        }
    }

    /// A pin listed as special must exist on the chip.
    #[test]
    fn the_tables_are_self_consistent() {
        for (name, caps) in CAPS {
            assert!(caps.pins > 0, "{name}");
            for &pin in caps
                .input_only
                .iter()
                .chain(caps.flash)
                .chain(caps.strapping)
            {
                assert!(
                    pin < caps.pins,
                    "{name}: GPIO{pin} is outside GPIO0..GPIO{}",
                    caps.pins - 1
                );
            }
        }
    }

    /// The pin counts this file documents, cross-checked against the table so a
    /// regenerated table that loses a pin is caught here.
    #[test]
    fn the_documented_pin_counts_match() {
        let expected = [
            ("esp32", 40),
            ("esp32c2", 21),
            ("esp32c3", 22),
            ("esp32c5", 29),
            ("esp32c6", 31),
            ("esp32c61", 30),
            ("esp32h2", 28),
            ("esp32p4", 55),
            ("esp32s2", 47),
            ("esp32s3", 49),
            ("esp32s31", 62),
        ];
        for (name, pins) in expected {
            assert_eq!(caps(name).expect(name).pins, pins, "{name}");
        }
    }

    #[test]
    fn an_unknown_chip_has_no_table() {
        assert!(caps("nonesuch").is_none());
    }
}

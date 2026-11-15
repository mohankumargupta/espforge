//! The default `partitions.csv` an `<ota />` project is flashed with.
//!
//! Two app slots (`ota_0`, `ota_1`) with an `otadata` sector between the NVS
//! and PHY partitions and the first slot. The slot size follows
//! [`FlashSize`]: half the flash, minus the 64 KiB the bootloader and the data
//! partitions occupy. A project that wants a different layout drops its own
//! `partitions.csv` beside the `.espf`; the overlay layer replaces this file
//! verbatim.

use espforge_elements::Meta;

/// The CSV header, exactly as `espflash` writes its own template.
const HEADER: &str = "# Name,   Type, SubType,  Offset,   Size,     Flags\n";

/// Offset of the first app slot: past the bootloader, partition table and the
/// three data partitions at the start of flash.
const OTA_0_OFFSET: u32 = 0x1_0000;

/// The emitted `partitions.csv` for `meta`'s `<ota />`.
///
/// `Meta` without `<ota />` never plans this file; the default keeps the
/// function total for a caller that asks anyway.
pub fn build(meta: &Meta) -> String {
    let flash_size = meta.ota.map(|ota| ota.flash_size).unwrap_or_default();
    let total = flash_size.bytes();
    // Half the flash, less the 64 KiB before the first slot. Already aligned
    // to 64 KiB for every declared size.
    let slot = total / 2 - OTA_0_OFFSET;
    let ota_1_offset = total / 2;

    let mut out = String::from(HEADER);
    out.push_str("nvs,      data, nvs,      0x9000,   0x4000,\n");
    out.push_str("otadata,  data, ota,      0xd000,   0x2000,\n");
    out.push_str("phy_init, data, phy,      0xf000,   0x1000,\n");
    out.push_str(&format!(
        "ota_0,    app,  ota_0,    0x10000,  0x{slot:X},\n"
    ));
    out.push_str(&format!(
        "ota_1,    app,  ota_1,    0x{ota_1_offset:X}, 0x{slot:X},\n"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_elements::{FlashSize, OtaMeta};

    fn meta(flash_size: FlashSize) -> Meta {
        Meta {
            name: "ota demo".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: espforge_elements::Runtime::Blocking,
            console: espforge_elements::Console::Uart,
            ota: Some(OtaMeta {
                flash_size,
                rollback: true,
            }),
            logger: None,
        }
    }

    /// The 4 MB layout the spec documents, byte for byte.
    #[test]
    fn the_default_4mb_layout_is_two_1_94_mib_slots() {
        assert_eq!(
            build(&meta(FlashSize::Mb4)),
            "# Name,   Type, SubType,  Offset,   Size,     Flags\n\
             nvs,      data, nvs,      0x9000,   0x4000,\n\
             otadata,  data, ota,      0xd000,   0x2000,\n\
             phy_init, data, phy,      0xf000,   0x1000,\n\
             ota_0,    app,  ota_0,    0x10000,  0x1F0000,\n\
             ota_1,    app,  ota_1,    0x200000, 0x1F0000,\n"
        );
    }

    /// Every declared size keeps both slots aligned to 64 KiB and leaves the
    /// second slot inside the flash.
    #[test]
    fn every_flash_size_leaves_room_for_two_aligned_slots() {
        for (flash_size, slot, second) in [
            (FlashSize::Mb2, 0xF0000, 0x100000),
            (FlashSize::Mb4, 0x1F0000, 0x200000),
            (FlashSize::Mb8, 0x3F0000, 0x400000),
            (FlashSize::Mb16, 0x7F0000, 0x800000),
        ] {
            assert_eq!(slot % 0x1_0000, 0, "slot of {flash_size:?} is aligned");
            assert_eq!(
                second + slot,
                flash_size.bytes() - 0x1_0000,
                "two slots fill flash past the 64 KiB head"
            );
            let csv = build(&meta(flash_size));
            assert!(
                csv.contains(&format!("0x10000,  0x{slot:X},")),
                "{flash_size:?}: {csv}"
            );
            assert!(
                csv.contains(&format!("0x{second:X}, 0x{slot:X},")),
                "{flash_size:?}: {csv}"
            );
        }
    }

    /// The data partitions `<ota>` depends on are always present.
    #[test]
    fn the_otadata_sector_is_present() {
        let csv = build(&meta(FlashSize::Mb4));
        assert!(csv.contains("otadata,  data, ota,      0xd000,   0x2000,"));
    }
}

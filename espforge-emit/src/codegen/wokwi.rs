//! `wokwi.toml`, built with `toml_edit`, including the `[[chip]]` section a
//! Wokwi custom chip needs.
//!
//! The file is only emitted when the `wokwi` option is on. `[[chip]]` depends
//! on example content rather than on the project: when an example ships
//! `chip.wasm` + `chip.json` (seeded at the project root), the `chip-<name>`
//! part in `diagram.json` only resolves if `wokwi.toml` declares it. The
//! section is appended as text so an example's own `wokwi.toml` keeps its
//! formatting; the name is parsed from `chip.json` (every example uses
//! `chip`, which is the fallback).

use anyhow::Result;
use espforge_elements::Meta;
use toml_edit::{DocumentMut, Item, Table, value};

use super::chip::Chip;

/// Wokwi's serial monitor part id, used by `diagram.json` connections.
pub const SERIAL_MONITOR: &str = "$serialMonitor";

/// Wokwi board id for a chip (this used to be an if-chain in the template).
pub fn board(chip: &str) -> &'static str {
    match chip {
        "esp32c3" => "board-esp32-c3-devkitm-1",
        "esp32c6" => "board-esp32-c6-devkitc-1",
        "esp32h2" => "board-esp32-h2-devkitm-1",
        "esp32s2" => "board-esp32-s2-devkitm-1",
        "esp32s3" => "board-esp32-s3-devkitc-1",
        // "esp32" and anything Wokwi has no dedicated board for.
        _ => "board-esp32-devkit-c-v4",
    }
}

/// The emitted `wokwi.toml`.
pub fn build(meta: &Meta, chip: &Chip) -> Result<String> {
    let firmware = format!("target/{}/debug/{}", chip.rust_target, meta.crate_name());
    let mut wokwi = Table::new();
    wokwi["version"] = value(1);
    wokwi["gdbServerPort"] = value(3333);
    wokwi["elf"] = value(firmware.as_str());
    wokwi["firmware"] = value(firmware.as_str());
    let mut doc = DocumentMut::new();
    doc["wokwi"] = Item::Table(wokwi);
    Ok(doc.to_string())
}

/// The `[[chip]]` section to append to the emitted `wokwi.toml`.
///
/// `chip_json` is the seeded `chip.json` contents when the example has one;
/// its `"name"` field becomes the wokwi part suffix, else `"chip"`. TOML
/// literal string quoting is used, which is safe for the identifier-style
/// names wokwi chips use. Both seeded files sit next to `wokwi.toml` at the
/// project root, and Wokwi pairs the JSON pin description with the wasm by
/// basename (`chip.wasm` / `chip.json`).
pub fn chip_section(chip_json: Option<&str>) -> String {
    let name = chip_json
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .unwrap_or_else(|| "chip".to_string());
    format!("\n[[chip]]\nname = '{name}'\nbinary = 'chip.wasm'\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::{case, chip};
    use espforge_elements::Runtime;

    fn build_wokwi(meta: &Meta, chip_name: &str) -> String {
        build(meta, &chip(chip_name)).unwrap()
    }

    #[test]
    fn the_firmware_paths_name_the_target_and_the_crate() {
        let case = case("esp32c3", Runtime::Blocking, true, &[], &[]);
        let doc: DocumentMut = build_wokwi(&case.meta, "esp32c3").parse().unwrap();
        assert_eq!(doc["wokwi"]["version"].as_integer(), Some(1));
        assert_eq!(doc["wokwi"]["gdbServerPort"].as_integer(), Some(3333));
        assert_eq!(
            doc["wokwi"]["elf"].as_str(),
            Some("target/riscv32imc-unknown-none-elf/debug/hello_world")
        );
        assert_eq!(
            doc["wokwi"]["firmware"].as_str(),
            doc["wokwi"]["elf"].as_str()
        );
    }

    #[test]
    fn the_board_table_matches_the_old_diagram_chain() {
        assert_eq!(board("esp32c3"), "board-esp32-c3-devkitm-1");
        assert_eq!(board("esp32"), "board-esp32-devkit-c-v4");
        assert_eq!(board("esp32s3"), "board-esp32-s3-devkitc-1");
        assert_eq!(board("esp32c61"), "board-esp32-devkit-c-v4");
    }

    #[test]
    fn an_xtensa_project_uses_the_xtensa_target() {
        let case = case("esp32s3", Runtime::Blocking, true, &[], &[]);
        assert!(
            build_wokwi(&case.meta, "esp32s3")
                .contains("target/xtensa-esp32s3-none-elf/debug/hello_world")
        );
    }

    #[test]
    fn the_chip_section_takes_its_name_from_chip_json() {
        let json = r#"{"name": "chip", "pins": ["SDA", "SCL"]}"#;
        assert_eq!(
            chip_section(Some(json)),
            "\n[[chip]]\nname = 'chip'\nbinary = 'chip.wasm'\n"
        );
        assert!(chip_section(Some(r#"{"name": "mcp23017"}"#)).contains("name = 'mcp23017'"));
    }

    #[test]
    fn a_missing_or_broken_chip_json_falls_back_to_chip() {
        assert_eq!(
            chip_section(None),
            "\n[[chip]]\nname = 'chip'\nbinary = 'chip.wasm'\n"
        );
        assert_eq!(chip_section(Some("not json")), chip_section(None));
    }
}

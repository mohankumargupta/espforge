//! Wokwi custom-chip glue: the `[[chip]]` section of `wokwi.toml`.
//!
//! In the old design this module wrote the whole `wokwi.toml` and
//! token-resolved `diagram.json` from an IR device tree. The new design
//! renders both through the template/overlay engine instead (`{{ board }}`
//! and `{{ serialMonitor }}` come from the context, `wokwi.toml` is a plain
//! template artifact that an example may replace), so the one piece left
//! here is the part that depends on example content: when an example ships
//! a Wokwi custom chip (`chip.wasm` + `chip.json`, seeded at the project
//! root), `wokwi.toml` needs a `[[chip]]` section so the `chip-<name>` part
//! in `diagram.json` resolves.
//!
//! The part type is `chip-<name>` where `name` is the `"name"` field of
//! `chip.json` (every example uses `chip`, so that is the fallback). Both
//! seeded files sit next to `wokwi.toml` at the project root, and Wokwi
//! pairs the JSON pin description with the wasm by basename (`chip.wasm` /
//! `chip.json`).

/// The `[[chip]]` section to append to the emitted `wokwi.toml`.
///
/// `chip_json` is the seeded `chip.json` contents when the example has one;
/// its `"name"` field becomes the wokwi part suffix, else `"chip"`. TOML
/// literal string quoting is used, which is safe for the identifier-style
/// names wokwi chips use.
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

    #[test]
    fn the_section_takes_its_name_from_chip_json() {
        let json = r#"{"name": "chip", "pins": ["SDA", "SCL"]}"#;
        assert_eq!(
            chip_section(Some(json)),
            "\n[[chip]]\nname = 'chip'\nbinary = 'chip.wasm'\n"
        );
        // and a non-default name flows through
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

//! `diagram.json`, built from typed structs and serialized with serde.
//!
//! This is the default diagram only (the board plus the serial monitor). An
//! example that ships its own `diagram.json` replaces it wholesale, with its
//! placeholders resolved by [`super::overlay`].

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

use super::chip::Chip;
use super::wokwi;

/// The emitted `diagram.json`.
pub fn build(chip: &Chip) -> Result<String> {
    let diagram = Diagram {
        version: 1,
        editor: "wokwi",
        parts: vec![Part {
            kind: wokwi::board(&chip.name).to_string(),
            id: "esp".to_string(),
            top: 0.59,
            left: 0.67,
            attrs: BTreeMap::from([("flashSize".to_string(), "16".to_string())]),
        }],
        connections: vec![
            Connection::power("esp:TX", format!("{}:RX", wokwi::SERIAL_MONITOR)),
            Connection::power("esp:RX", format!("{}:TX", wokwi::SERIAL_MONITOR)),
        ],
        serial_monitor: SerialMonitor {
            display: "terminal",
            convert_eol: true,
        },
    };
    Ok(serde_json::to_string_pretty(&diagram)? + "\n")
}

#[derive(Debug, Serialize)]
struct Diagram {
    version: u32,
    editor: &'static str,
    parts: Vec<Part>,
    connections: Vec<Connection>,
    #[serde(rename = "serialMonitor")]
    serial_monitor: SerialMonitor,
}

#[derive(Debug, Serialize)]
struct Part {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    top: f64,
    left: f64,
    attrs: BTreeMap<String, String>,
}

/// A Wokwi connection: two pins, a colour and the wire's waypoints. Serialized
/// as the four-element array the format uses.
#[derive(Debug, Serialize)]
struct Connection(String, String, String, Vec<String>);

impl Connection {
    fn power(from: &str, to: String) -> Self {
        Self(from.to_string(), to, String::new(), Vec::new())
    }
}

#[derive(Debug, Serialize)]
struct SerialMonitor {
    display: &'static str,
    #[serde(rename = "convertEol")]
    convert_eol: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::chip;
    use serde_json::Value;

    fn diagram(chip_name: &str) -> Value {
        serde_json::from_str(&build(&chip(chip_name)).unwrap()).unwrap()
    }

    #[test]
    fn the_default_diagram_is_the_board_and_the_serial_monitor() {
        let json = diagram("esp32c3");
        assert_eq!(json["version"], 1);
        assert_eq!(json["editor"], "wokwi");
        assert_eq!(json["parts"][0]["type"], "board-esp32-c3-devkitm-1");
        assert_eq!(json["parts"][0]["id"], "esp");
        assert_eq!(json["parts"][0]["attrs"]["flashSize"], "16");
        assert_eq!(json["connections"][0][0], "esp:TX");
        assert_eq!(json["connections"][0][1], "$serialMonitor:RX");
        assert_eq!(json["connections"][1][1], "$serialMonitor:TX");
        assert_eq!(json["serialMonitor"]["display"], "terminal");
        assert_eq!(json["serialMonitor"]["convertEol"], true);
    }

    #[test]
    fn the_board_follows_the_chip() {
        assert_eq!(
            diagram("esp32s3")["parts"][0]["type"],
            "board-esp32-s3-devkitc-1"
        );
    }

    #[test]
    fn the_file_ends_with_a_newline() {
        let text = build(&chip("esp32c3")).unwrap();
        assert!(text.ends_with("}\n"), "{text}");
    }
}

use std::collections::HashMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BoardConfig {
    pub board: BoardInfo,
}

#[derive(Debug, Deserialize)]
pub struct BoardInfo {
    pub name: String,
    pub chip: String,
    pub special_pins: HashMap<String, u8>,
    pub pins: HashMap<String, u8>,
}

// usage
pub fn load_board() {
    let toml_str = r#" ... (the toml above) ... "#;
    let config: BoardConfig = toml::from_str(toml_str).unwrap();

    // Access the NeoPixel pin
    if let Some(&pin) = config.board.special_pins.get("NEOPIXEL") {
        println!("NeoPixel is on GPIO {}", pin);
    }
}
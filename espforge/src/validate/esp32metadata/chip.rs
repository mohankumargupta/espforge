use std::collections::HashMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ChipConfig {
    esp32c3: ChipDefinition,
}

#[derive(Debug, Deserialize)]
struct ChipDefinition {
    name: String,
    gpio_limits: Limits,
    strapping_pins: Vec<u8>,
    reserved_pins: HashMap<String, String>, // Maps "12" -> "SPIHD"
}

#[derive(Debug, Deserialize)]
struct Limits {
    min: u8,
    max: u8,
}


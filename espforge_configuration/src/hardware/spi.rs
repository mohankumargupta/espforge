use super::ResolvePeripheral;
use crate::EspforgeConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SpiConfig {
    #[serde(default)]
    pub spi: u8,
    #[serde(default)]
    pub miso: Option<u8>,
    pub mosi: u8,
    pub sck: u8,
    #[serde(default)]
    pub cs: Option<u8>,
    #[serde(
        default = "default_spi_frequency",
        alias = "frequency_kHz",
        alias = "frequency_khz"
    )]
    pub frequency: u32,
    #[serde(default)]
    pub mode: u8,
}

fn default_spi_frequency() -> u32 {
    1000
}

pub struct SpiRef<'a>(pub &'a str);

impl<'a> AsRef<str> for SpiRef<'a> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl<'a> ResolvePeripheral<'a> for SpiRef<'a> {
    type Config = SpiConfig;

    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>> {
        Some(&root.esp32.as_ref()?.spi)
    }

    fn as_str(&self) -> &str {
        self.0
    }
    fn section_name() -> &'static str {
        "spi"
    }
}

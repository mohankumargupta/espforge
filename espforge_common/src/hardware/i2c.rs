use super::ResolvePeripheral;
use crate::EspforgeConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct I2cConfig {
    #[serde(default)]
    pub i2c: u8,
    pub sda: u8,
    pub scl: u8,
    #[serde(
        default = "default_i2c_frequency",
        alias = "frequency_kHz",
        alias = "frequency_khz"
    )]
    pub frequency: u32,
}

fn default_i2c_frequency() -> u32 {
    100
}

pub struct I2cRef<'a>(pub &'a str);

impl<'a> AsRef<str> for I2cRef<'a> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl<'a> ResolvePeripheral<'a> for I2cRef<'a> {
    type Config = I2cConfig;

    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>> {
        Some(&root.esp32.as_ref()?.i2c)
    }

    fn as_str(&self) -> &str {
        self.0
    }
    fn section_name() -> &'static str {
        "i2c"
    }
}

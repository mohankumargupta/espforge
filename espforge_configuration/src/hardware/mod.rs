use crate::EspforgeConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub mod gpio;
pub mod heap;
pub mod i2c;
pub mod psram;
pub mod spi;
pub mod uart;
pub mod wifi;

use heap::HeapConfig;

#[derive(Error, Debug)]
pub enum ResolutionError {
    #[error("Reference '{0}' is invalid: missing '$' prefix")]
    InvalidReference(String),
    #[error("Configuration section 'esp32.{0}' is missing or empty")]
    MissingSection(String),
    #[error("Resource '{name}' not found in 'esp32.{section}'. Available: {available:?}")]
    ResourceNotFound {
        name: String,
        section: String,
        available: Vec<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Esp32Config {
    #[serde(default)]
    pub gpio: HashMap<String, gpio::GpioPinConfig>,
    #[serde(default)]
    pub i2c: HashMap<String, i2c::I2cConfig>,
    #[serde(default)]
    pub spi: HashMap<String, spi::SpiConfig>,
    #[serde(default)]
    pub uart: HashMap<String, uart::UartConfig>,
    #[serde(default)]
    pub psram: Option<psram::PsramConfig>,
    #[serde(default)]
    pub wifi: Option<wifi::WifiConfig>,
    /// Optional heap size override.
    pub heap: Option<HeapConfig>,
}

pub trait ResolvePeripheral<'a>: AsRef<str> {
    type Config;
    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>>;
    fn section_name() -> &'static str;
    fn as_str(&self) -> &str;
}

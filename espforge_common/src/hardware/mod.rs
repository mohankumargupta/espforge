use crate::EspforgeConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;

pub use gpio::{GpioPinConfig, GpioRef, PinDirection};
pub use i2c::{I2cConfig, I2cRef};
pub use spi::{SpiConfig, SpiRef};
pub use uart::{UartConfig, UartRef};

#[derive(Debug, Error)]
pub enum ResolutionError {
    #[error("Reference '{0}' is invalid: missing '$' prefix")]
    InvalidPrefix(String),

    #[error("Configuration section 'esp32.{0}' is missing or empty")]
    MissingSection(&'static str),

    #[error("Resource '{name}' not found in 'esp32.{section}'. Available: {available:?}")]
    NotFound {
        name: String,
        section: &'static str,
        available: Vec<String>,
    },
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Esp32Config {
    #[serde(default)]
    pub gpio: HashMap<String, GpioPinConfig>,
    #[serde(default)]
    pub spi: HashMap<String, SpiConfig>,
    #[serde(default)]
    pub i2c: HashMap<String, I2cConfig>,
    #[serde(default)]
    pub uart: HashMap<String, UartConfig>,
}

pub trait ResolvePeripheral<'a>: AsRef<str> {
    type Config;

    /// Returns the map containing the configs for this peripheral type
    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>>;

    fn as_str(&self) -> &str;
    fn section_name() -> &'static str;
}
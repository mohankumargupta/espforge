#[cfg(feature = "std")]
use serde_yaml_ng::Value;

pub mod led;
pub mod button;
pub mod i2c;
pub mod spi;
pub mod uart;

#[cfg(feature = "std")]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Component {
    #[serde(alias = "using")]
    pub driver: String,
    #[serde(default, alias = "with")]
    pub properties: Value,
}

#[cfg(feature = "std")]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Device {
    #[serde(alias = "using")]
    pub driver: String,
    #[serde(default, alias = "with")]
    pub properties: Value,
}

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(feature = "std")]
pub mod codegen;
pub mod components;
#[cfg(feature = "std")]
pub mod config;
#[cfg(feature = "std")]
pub mod dependency;
#[cfg(feature = "std")]
pub mod hardware;
#[cfg(feature = "std")]
pub mod plugin;

#[cfg(feature = "std")]
pub use config::ConfigParser;

#[cfg(feature = "std")]
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct EspforgeConfiguration {
    #[serde(default)]
    pub name: String,
    
    #[serde(default)]
    pub chip: String,

    #[serde(default)]
    pub esp32: Option<hardware::Esp32Config>,

    #[serde(default)]
    pub components: HashMap<String, components::Component>,

    #[serde(default)]
    pub devices: HashMap<String, components::Device>,
}

#[cfg(feature = "std")]
impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        if self.name.is_empty() {
            "espforge-project"
        } else {
            &self.name
        }
    }

    pub fn get_chip(&self) -> &str {
        if self.chip.is_empty() {
            "esp32c3"
        } else {
            &self.chip
        }
    }
}

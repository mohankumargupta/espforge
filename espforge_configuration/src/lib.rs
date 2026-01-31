use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub mod hardware;
pub mod config;
pub mod plugin;
pub mod components;

pub use config::ConfigParser;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EspforgeConfiguration {
    #[serde(default)]
    pub espforge: HashMap<String, String>,
    
    #[serde(default)]
    pub esp32: Option<hardware::Esp32Config>,
    
    #[serde(default)]
    pub components: HashMap<String, components::Component>,
    
    #[serde(default)]
    pub devices: HashMap<String, components::Device>,
}

impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        self.espforge.get("name").map(|s| s.as_str()).unwrap_or("espforge-project")
    }

    pub fn get_chip(&self) -> &str {
        self.espforge.get("platform").or_else(|| self.espforge.get("chip")).map(|s| s.as_str()).unwrap_or("esp32")
    }
}

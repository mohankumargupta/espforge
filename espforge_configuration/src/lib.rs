use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod components;
pub mod config;
pub mod hardware;
pub mod plugin;

pub use config::ConfigParser;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct EspforgeConfiguration {
    pub espforge: HashMap<String, String>,
    pub esp32: Option<hardware::Esp32Config>,
    pub components: HashMap<String, components::Component>,
    pub devices: HashMap<String, components::Device>,
}

#[derive(Default, Debug, Clone, PartialEq)]
pub enum RuntimeMode {
    #[default]
    None,
    Embassy,
}

impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        self.espforge
            .get("name")
            .map(|s| s.as_str())
            .unwrap_or("espforge-project")
    }

    pub fn get_chip(&self) -> &str {
        self.espforge
            .get("platform")
            .or_else(|| self.espforge.get("chip"))
            .map(|s| s.as_str())
            .unwrap_or("esp32")
    }

    pub fn get_runtime(&self) -> RuntimeMode {
        self.espforge
            .get("runtime")
            .map(|s| s.as_str())
            .and_then(|s| {
                if s == "embassy" {
                    Some(RuntimeMode::Embassy)
                } else {
                    None
                }
            })
            .unwrap_or(RuntimeMode::None)
    }

    pub fn is_embassy(&self) -> bool {
        matches!(self.get_runtime(), RuntimeMode::Embassy)
    }

    pub fn runtime_name(&self) -> &'static str {
        match self.get_runtime() {
            RuntimeMode::Embassy => "embassy",
            RuntimeMode::None => "blocking",
        }
    }
}

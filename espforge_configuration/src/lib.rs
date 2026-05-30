use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod components;
pub mod config;
pub mod hardware;
pub mod plugin;

pub use config::ConfigParser;

pub mod refs;
pub use refs::{ComponentRef, DeviceRef, PinRef};

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

    /// Check if heap allocation is enabled
    pub fn has_alloc(&self) -> bool {
        // self.espforge
        //     .get("alloc")
        //     .map(|s| s == "true")
        //     .unwrap_or(false)
    let explicit = self.espforge.get("alloc").map(|s| s == "true").unwrap_or(false);
    let has_heap_block = self.esp32.as_ref().and_then(|e| e.heap.as_ref()).is_some();
    explicit || has_heap_block
    }

    /// Get heap size for the configured chip
    /// Returns None if alloc is not enabled or chip not found
    pub fn get_heap_size(&self) -> Option<usize> {
        if !self.has_alloc() {
            return None;
        }

    // 1. Explicit YAML override
    if let Some(size) = self.esp32.as_ref()
        .and_then(|e| e.heap.as_ref())
        .map(|h| h.size)
    {
        return Some(size);
    }


        // Load chip database and get heap size for the configured chip
        let db = espforge_esp32metadata::BoardDatabase::load();
        db.max_heap_size(self.get_chip())
    }

    pub fn has_psram(&self) -> Option<bool> {
        self.esp32.as_ref().map(|s| s.psram.is_some())
    }

    pub fn has_wifi(&self) -> bool {
        self.esp32.as_ref().and_then(|e| e.wifi.as_ref()).is_some()
    }
}

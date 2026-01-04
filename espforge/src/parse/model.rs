use std::collections::HashMap;
use serde_yaml_ng::Value;

/// Represents the in-memory state of a parsed project configuration.
/// 
/// This struct is populated by various `SectionProcessor` implementations
/// during the parsing phase.
#[derive(Debug, Default)]
pub struct ProjectModel {
    /// The project name (from `espforge.name`)
    pub name: String,
    
    /// The target chip (from `espforge.platform`)
    pub chip: String,
    
    /// GPIO definitions (from `esp32.gpio`)
    /// Maps an alias (e.g., "gpio2") to its configuration
    pub gpios: HashMap<String, GpioConfig>,
    
    /// Component definitions (from `components`)
    /// Maps a component name (e.g., "red_led") to its configuration
    pub components: HashMap<String, ComponentConfig>,
}

impl ProjectModel {
    /// Helper to get the project name, or default if not set
    pub fn get_name(&self) -> &str {
        if self.name.is_empty() {
            "espforge_project"
        } else {
            &self.name
        }
    }
    
    /// Helper to get the target chip
    pub fn get_chip(&self) -> &str {
        &self.chip
    }
}

/// Configuration for a specific GPIO pin
#[derive(Debug, Clone)]
pub struct GpioConfig {
    pub pin: u8,
    pub direction: String, // e.g., "output", "input"
}

/// Configuration for a high-level component
#[derive(Debug, Clone)]
pub struct ComponentConfig {
    /// The driver/class to use (e.g., "LED")
    pub using: String,
    
    /// Properties injected into the component (e.g., gpio mappings)
    pub properties: HashMap<String, Value>,
}
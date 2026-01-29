use anyhow::{Context, Result};
use espforge_common::EspforgeConfiguration;
use serde_yaml_ng::Value;

use crate::parse::{
    processor::SectionProcessor,
};

pub mod components;
pub mod devices;
pub mod esp32;
pub mod model;
pub mod processor;
pub mod project;


pub struct ConfigParser {
    processors: Vec<Box<dyn SectionProcessor>>,
}

impl ConfigParser {
    /// Helper to start the building process
    pub fn builder() -> ConfigParserBuilder {
        ConfigParserBuilder::default()
    }

    /// Helper to get a parser with standard defaults immediately
    pub fn new() -> Self {
        ConfigParserBuilder::default_processors().build()
    }

    // The logic stays here, but the name implies action now.
    pub fn parse(&self, yaml_text: &str) -> Result<EspforgeConfiguration> {
        let raw_yaml: Value = serde_yaml_ng::from_str(yaml_text)?;
        let root_map = raw_yaml
            .as_mapping()
            .ok_or_else(|| anyhow::anyhow!("Config must be a map"))?;

        let mut model = EspforgeConfiguration::default();

        for processor in &self.processors {
            let key = processor.section_key();
            if let Some(section_content) = root_map.get(Value::String(key.to_string())) {
                processor
                    .process(section_content, &mut model)
                    .with_context(|| format!("Error processing configuration section '{}'", key))?;
            }
        }

        if model.chip.is_empty() {
            return Err(anyhow::anyhow!(
                "Project configuration missing required 'espforge.chip' or 'espforge.platform'"
            ));
        }

        Ok(model)
    }
}


pub struct ConfigParserBuilder {
    processors: Vec<Box<dyn SectionProcessor>>,
}

impl Default for ConfigParserBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigParserBuilder {
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
        }
    }

    pub fn default_processors() -> Self {
        Self::new()
            .with_processor(Box::new(project::ProjectInfoProvisioner))
            .with_processor(Box::new(esp32::PlatformProvisioner))
            .with_processor(Box::new(components::ComponentProvisioner))
            .with_processor(Box::new(devices::DeviceProvisioner))
    }

    pub fn with_processor(mut self, processor: Box<dyn SectionProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    // Returns the Parser, not another Builder
    pub fn build(mut self) -> ConfigParser {
        self.processors.sort_by_key(|p| std::cmp::Reverse(p.priority()));

        ConfigParser {
            processors: self.processors,
        }
    }
}

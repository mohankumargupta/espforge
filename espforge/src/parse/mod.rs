use crate::parse::processor::SectionProcessor;
use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use serde_yaml_ng::Value;

pub mod components;
pub mod devices;
pub mod esp32;
pub mod model;
pub mod processor;
pub mod project;

pub struct ConfigParser {
    processors: Vec<Box<dyn SectionProcessor>>,
}

impl Default for ConfigParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigParser {
    pub fn builder() -> ConfigParserBuilder {
        ConfigParserBuilder::default()
    }

    pub fn new() -> Self {
        ConfigParserBuilder::default_processors().build()
    }

    pub fn parse(&self, yaml_text: &str) -> Result<EspforgeConfiguration> {
        let root_value: Value =
            serde_yaml_ng::from_str(yaml_text).context("Failed to parse YAML")?;

        let root_map = root_value
            .as_mapping()
            .ok_or_else(|| anyhow::anyhow!("Config must be a map"))?;

        let mut model = EspforgeConfiguration::default();

        for processor in &self.processors {
            let key = processor.section_key();
            // Value::String key lookup in serde_yaml_ng Mapping
            if let Some(section_content) = root_map.get(Value::String(key.to_string())) {
                processor
                    .process(section_content, &mut model)
                    .with_context(|| format!("Error processing configuration section '{}'", key))?;
            }
        }

        // Validate chip existence by checking the map
        if !model.espforge.contains_key("platform") && !model.espforge.contains_key("chip") {
            return Err(anyhow::anyhow!(
                "Config must specify a chip/platform (e.g., 'platform: esp32c3')"
            ));
        }

        Ok(model)
    }
}

#[derive(Default)]
pub struct ConfigParserBuilder {
    processors: Vec<Box<dyn SectionProcessor>>,
}

impl ConfigParserBuilder {
    pub fn default_processors() -> Self {
        Self::default()
            .with_processor(Box::new(project::ProjectInfoProvisioner))
            .with_processor(Box::new(esp32::PlatformProvisioner))
            .with_processor(Box::new(components::ComponentProvisioner))
            .with_processor(Box::new(devices::DeviceProvisioner))
    }

    pub fn with_processor(mut self, processor: Box<dyn SectionProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub fn build(mut self) -> ConfigParser {
        self.processors
            .sort_by_key(|p| std::cmp::Reverse(p.priority()));
        ConfigParser {
            processors: self.processors,
        }
    }
}

use anyhow::{Context, Result};
use serde_yaml_ng::Value;

use crate::parse::{
    model::ProjectModel,
    processor::{ProcessorRegistration, SectionProcessor},
};

pub mod components;
pub mod esp32;
pub mod model;
pub mod processor;
pub mod project;

pub struct ConfigurationOrchestrator;

impl Default for ConfigurationOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurationOrchestrator {
    pub fn new() -> Self {
        Self {}
    }

    pub fn compile(&self, yaml_text: &str) -> Result<ProjectModel> {
        let raw_yaml: Value = serde_yaml_ng::from_str(yaml_text)?;
        let root_map = raw_yaml
            .as_mapping()
            .ok_or_else(|| anyhow::anyhow!("Config must be a map"))?;

        let mut model = ProjectModel::default();

        let mut processors: Vec<Box<dyn SectionProcessor>> =
            inventory::iter::<ProcessorRegistration>
                .into_iter()
                .map(|reg| (reg.factory)())
                .collect();

        processors.sort_by_key(|b| std::cmp::Reverse(b.priority()));
        for processor in processors {
            let key = processor.section_key();
            if let Some(section_content) = root_map.get(Value::String(key.to_string())) {
                processor
                    .process(section_content, &mut model)
                    .with_context(|| format!("Error processing configuration section '{}'", key))?;
            }
        }

        if model.chip.is_empty() {
            return Err(anyhow::anyhow!(
                "Project configuration missing required 'espforge.chip' or 'espforge.platform' definition"
            ));
        }

        Ok(model)
    }
}

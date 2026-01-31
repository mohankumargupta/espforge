use crate::parse::processor::SectionProcessor;
use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use serde::Deserialize;
use serde_yaml_ng::Value;

#[derive(Deserialize)]
struct ProjectConfig {
    name: String,
    chip: Option<String>,
    platform: Option<String>,
}

pub struct ProjectInfoProvisioner;

impl SectionProcessor for ProjectInfoProvisioner {
    fn section_key(&self) -> &'static str {
        "espforge"
    }

    fn priority(&self) -> u32 {
        1000
    }

    fn process(&self, content: &Value, model: &mut EspforgeConfiguration) -> Result<()> {
        let config: ProjectConfig = serde_yaml_ng::from_value(content.clone())
            .context("Failed to deserialize espforge configuration")?;

        // Store name in the map
        model.espforge.insert("name".to_string(), config.name);

        // Store chip/platform in the map
        if let Some(chip) = config.platform.or(config.chip) {
            model.espforge.insert("platform".to_string(), chip);
        }

        println!("✓ Project metadata provisioned");
        Ok(())
    }
}

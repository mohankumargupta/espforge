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
    runtime: Option<String>,
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

        model.espforge.insert("name".to_string(), config.name);

        if let Some(chip) = config.platform.or(config.chip) {
            model.espforge.insert("platform".to_string(), chip);
        }

        if let Some(runtime) = config.runtime {
            if runtime == "embassy" {
                model.espforge.insert("runtime".to_string(), runtime);
            } 
        }

        println!("✓ Project metadata provisioned");
        Ok(())
    }
}

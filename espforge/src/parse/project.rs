use crate::parse::ProjectModel;
use crate::parse::processor::{ProcessorRegistration, SectionProcessor};
use anyhow::{Context, Result};
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

    fn process(&self, content: &Value, model: &mut ProjectModel) -> Result<()> {
        let config: ProjectConfig = serde_yaml_ng::from_value(content.clone())
            .context("Failed to deserialize espforge configuration")?;

        model.name = config.name;
        
        if let Some(chip) = config.chip {
            model.chip = chip;
        } else if let Some(platform) = config.platform {
            model.chip = platform;
        }

        println!("✓ Project metadata provisioned");

        Ok(())
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(ProjectInfoProvisioner),
    }
}
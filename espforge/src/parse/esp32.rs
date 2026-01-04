use crate::parse::processor::{SectionProcessor, ProcessorRegistration};
use crate::parse::ProjectModel;
use serde_yaml_ng::Value;
use anyhow::Result;

pub struct PlatformProvisioner;

impl SectionProcessor for PlatformProvisioner {
    fn section_key(&self) -> &'static str { "esp32" }
    fn priority(&self) -> u32 { 300 } 
    fn process(&self, content: &Value, model: &mut ProjectModel) -> Result<()> {
        println!("Provisioning ESP32...");
        Ok(())
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(PlatformProvisioner),
    }
}


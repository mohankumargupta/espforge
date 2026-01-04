use serde_yaml_ng::Value;

use crate::parse::{model::ProjectModel, processor::{ProcessorRegistration, SectionProcessor}};
use anyhow::Result;

pub struct DriverAssembler;

impl SectionProcessor for DriverAssembler {
    fn section_key(&self) -> &'static str { "components" }
    fn priority(&self) -> u32 { 200 } 
    fn process(&self, content: &Value, model: &mut ProjectModel) -> Result<()> {
        println!("Assembling Drivers...");
        Ok(())
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(DriverAssembler),
    }
}

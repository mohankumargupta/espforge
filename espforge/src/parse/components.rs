use serde_yaml_ng::Value;

use crate::compile::processor::{SectionProcessor, ProcessorRegistration};

pub struct DriverAssembler;

impl SectionProcessor for DriverAssembler {
    fn section_key(&self) -> &'static str { "components" }
    fn priority(&self) -> u8 { 200 } 
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

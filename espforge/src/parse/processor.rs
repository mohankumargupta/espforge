use anyhow::Result;
use espforge_configuration::EspforgeConfiguration;
use serde_yaml_ng::Value;

pub trait SectionProcessor {
    fn section_key(&self) -> &'static str;
    fn priority(&self) -> u32 {
        50
    }
    fn process(&self, content: &Value, model: &mut EspforgeConfiguration) -> Result<()>;
}

pub struct ProcessorRegistration {
    pub factory: fn() -> Box<dyn SectionProcessor>,
}

inventory::collect!(ProcessorRegistration);

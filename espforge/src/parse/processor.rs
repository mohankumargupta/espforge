use anyhow::Result;
use serde_yaml_ng::Value;

use crate::parse::model::ProjectModel;

pub trait SectionProcessor {
    fn section_key(&self) -> &'static str;
    fn priority(&self) -> u32 {
        50
    } // Default priority
    fn process(&self, content: &Value, model: &mut ProjectModel) -> Result<()>;
}

pub struct ProcessorRegistration {
    pub factory: fn() -> Box<dyn SectionProcessor>,
}

inventory::collect!(ProcessorRegistration);

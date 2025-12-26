use crate::manifest::ParameterType;
use crate::register_strategy;
use crate::resolver::strategies::{ParameterStrategy, ResolutionContext};
use anyhow::{Result, anyhow};
use espforge_macros::auto_register_param_strategy;
use serde_yaml_ng::Value;

/// Strategy for resolving I2C Component references.
#[derive(Default)]
#[auto_register_param_strategy(ParameterType::I2cComponentRef, ParameterType::SpiComponentRef)]
pub struct ComponentRefStrategy;

impl ParameterStrategy for ComponentRefStrategy {
    fn resolve(&self, value: &Value, _ctx: &ResolutionContext) -> Result<Value> {
        let ref_name = self.extract_name(value)?;
        Ok(Value::String(ref_name.to_string()))
    }
}

impl ComponentRefStrategy {
    fn extract_name<'a>(&self, value: &'a Value) -> Result<&'a str> {
        let val_str = value
            .as_str()
            .ok_or_else(|| anyhow!("Component reference must be a string"))?;

        val_str
            .strip_prefix('$')
            .ok_or_else(|| anyhow!("Component reference must start with '$', got: {}", val_str))
    }
}

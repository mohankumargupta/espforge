use crate::register_strategy;
use crate::resolver::strategies::{ParameterStrategy, ResolutionContext};
use crate::resolver::strategies::utils::resolve_hardware_resource;
use crate::manifest::ParameterType;
use anyhow::Result;
use espforge_macros::auto_register_param_strategy;
use serde_yaml_ng::Value;

#[derive(Default)]
#[auto_register_param_strategy(ParameterType::UartRef)]
pub struct UartStrategy;

impl ParameterStrategy for UartStrategy {
    fn resolve(&self, value: &Value, ctx: &ResolutionContext) -> Result<Value> {
        resolve_hardware_resource(
            value,
            ctx,
            |hw| &hw.uart,
            |c| c.clone()
        )
    }
}


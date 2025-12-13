use crate::config::{Esp32Config, PlatformConfig};
use crate::manifest::ParameterType;
use anyhow::Result;
use espforge_macros::auto_register_param_strategy;
use inventory;
use serde_yaml_ng::Value;

pub mod component;
pub mod hardware;
pub struct ResolutionContext<'a> {
    pub platform: &'a PlatformConfig,
    pub hardware: Option<&'a Esp32Config>,
}

pub trait ParameterStrategy: Send + Sync {
    fn resolve(&self, value: &Value, ctx: &ResolutionContext) -> Result<Value>;
}

pub type StrategyFactory = fn() -> (ParameterType, Box<dyn ParameterStrategy>);

pub struct StrategyRegistration {
    pub factory: StrategyFactory,
}

inventory::collect!(StrategyRegistration);

#[macro_export]
macro_rules! register_strategy {
    ($param_type:expr, $strategy:ty) => {
        inventory::submit! {
            $crate::resolver::strategies::StrategyRegistration {
                factory: || ($param_type, Box::new(<$strategy>::default()))
            }
        }
    };
}

#[derive(Default)]
#[auto_register_param_strategy(
    ParameterType::String,
    ParameterType::Integer,
    ParameterType::Boolean
)]
pub struct PrimitiveStrategy;

impl ParameterStrategy for PrimitiveStrategy {
    fn resolve(&self, value: &Value, _ctx: &ResolutionContext) -> Result<Value> {
        Ok(value.clone())
    }
}

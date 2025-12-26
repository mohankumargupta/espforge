use crate::register_strategy;
use crate::resolver::ParameterType;
use crate::resolver::strategies::{ParameterStrategy, ResolutionContext, ValueExt};
use crate::resolver::strategies::views::SpiTemplateView;
use anyhow::{Result, anyhow};
use espforge_macros::auto_register_param_strategy;
use serde_yaml_ng::Value;

#[derive(Default)]
#[auto_register_param_strategy(
    ParameterType::GpioRef,
    ParameterType::SpiRef,
    ParameterType::I2cRef,
    ParameterType::UartRef
)]
pub struct HardwareStrategy;

impl ParameterStrategy for HardwareStrategy {
    fn resolve(&self, value: &Value, ctx: &ResolutionContext) -> Result<Value> {
        let name = value.as_ref_name()?;
        let hardware = ctx.hardware()?;

        hardware.gpio.get(name)
            .map(|c| serde_yaml_ng::to_value(c).map_err(|e| anyhow::Error::from(e)))
            .or_else(|| {
                hardware.spi.get(name).map(|c| {
                    let view = SpiTemplateView::from(c);
                    serde_yaml_ng::to_value(view).map_err(|e| anyhow::Error::from(e))
                })
            })
            .or_else(|| hardware.i2c.get(name).map(|c| serde_yaml_ng::to_value(c).map_err(|e| anyhow::Error::from(e))))
            .or_else(|| hardware.uart.get(name).map(|c| serde_yaml_ng::to_value(c).map_err(|e| anyhow::Error::from(e))))
            .ok_or_else(|| {
                anyhow!(
                    "Undefined Hardware Reference: '{}'",
                    name
                )
            })?
    }
}
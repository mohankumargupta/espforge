use crate::resolver::strategies::{ParameterStrategy, ResolutionContext};
use crate::register_strategy;
use crate::resolver::ParameterType;
use anyhow::{Result, anyhow};
use espforge_macros::auto_register_param_strategy;
use serde_yaml_ng::Value;

/// Concrete Strategy for GPIO References
///
/// Resolves GPIO references like "$LED_PIN" to actual pin numbers
/// by looking up the hardware configuration
#[derive(Default)]
#[auto_register_param_strategy(ParameterType::GpioRef)]
pub struct GpioStrategy;

impl ParameterStrategy for GpioStrategy {
    fn resolve(&self, value: &Value, ctx: &ResolutionContext) -> Result<Value> {
        let gpio_name = self.extract_gpio_name(value)?;
        let pin_config = self.lookup_pin_config(gpio_name, ctx)?;
        Ok(self.create_pin_mapping(pin_config))
    }
}

impl GpioStrategy {
    fn extract_gpio_name<'a>(&self, value: &'a Value) -> Result<&'a str> {
        let val_str = value
            .as_str()
            .ok_or_else(|| anyhow!("GPIO value must be a string"))?;

        val_str
            .strip_prefix('$')
            .ok_or_else(|| anyhow!("GPIO reference must start with '$', got: {}", val_str))
    }

    fn lookup_pin_config<'a>(&self, gpio_name: &str, ctx: &'a ResolutionContext) -> Result<&'a crate::config::GpioPinConfig> {
        let hardware = ctx.hardware.ok_or_else(|| {
            anyhow!("Hardware configuration (esp32) is required for GPIO references")
        })?;

        hardware.gpio.get(gpio_name)
            .ok_or_else(|| anyhow!("Undefined GPIO: '{}'", gpio_name))
    }

    fn create_pin_mapping(&self, config: &crate::config::GpioPinConfig) -> Value {
        let mut map = serde_yaml_ng::Mapping::new();
        map.insert(Value::from("pin"), Value::from(config.pin));
        map.insert(Value::from("pullup"), Value::from(config.pullup));
        map.insert(Value::from("pulldown"), Value::from(config.pulldown));
        Value::Mapping(map)
    }
}

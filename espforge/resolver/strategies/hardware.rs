use crate::register_strategy;
use crate::resolver::ParameterType;
use crate::resolver::strategies::{ParameterStrategy, ResolutionContext};
use anyhow::{Result, anyhow};
use espforge_macros::auto_register_param_strategy;
use serde_yaml_ng::Value;

/// references in yaml configuration
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
        let ref_name = self.extract_name(value)?;

        let hardware = ctx.hardware.ok_or_else(|| {
            anyhow!("Hardware configuration (esp32) is required for hardware references")
        })?;

        // Try to find it in GPIO
        if let Some(gpio_config) = hardware.gpio.get(ref_name) {
            let mut map = serde_yaml_ng::Mapping::new();
            map.insert(Value::from("pin"), Value::from(gpio_config.pin));
            map.insert(Value::from("pullup"), Value::from(gpio_config.pullup));
            map.insert(Value::from("pulldown"), Value::from(gpio_config.pulldown));
            return Ok(Value::Mapping(map));
        }

        // Try to find it in SPI
        if let Some(spi_config) = hardware.spi.get(ref_name) {
            let mut map = serde_yaml_ng::Mapping::new();
            map.insert(Value::from("spi"), Value::from(spi_config.spi));
            map.insert(Value::from("miso"), Value::from(spi_config.miso));
            map.insert(Value::from("mosi"), Value::from(spi_config.mosi));
            map.insert(Value::from("sck"), Value::from(spi_config.sck));
            map.insert(Value::from("frequency"), Value::from(spi_config.frequency));
            map.insert(Value::from("mode"), Value::from(spi_config.mode));
            if let Some(cs) = spi_config.cs {
                map.insert(Value::from("cs"), Value::from(cs));
            } else {
                map.insert(Value::from("cs"), Value::from(u8::MAX)); // Indicator for no CS
            }
            return Ok(Value::Mapping(map));
        }

        if let Some(i2c_config) = hardware.i2c.get(ref_name) {
             return serde_yaml_ng::to_value(i2c_config).map_err(|e| anyhow::anyhow!(e));
        }

        if let Some(uart_config) = hardware.uart.get(ref_name) {
            return Ok(uart_config.clone());
        }

        Err(anyhow!(
            "Undefined Hardware Reference: '${}' (checked gpio, spi)",
            ref_name
        ))
    }
}

impl HardwareStrategy {
    fn extract_name<'a>(&self, value: &'a Value) -> Result<&'a str> {
        let val_str = value
            .as_str()
            .ok_or_else(|| anyhow!("Reference value must be a string"))?;

        val_str
            .strip_prefix('$')
            .ok_or_else(|| anyhow!("Hardware reference must start with '$', got: {}", val_str))
    }
}


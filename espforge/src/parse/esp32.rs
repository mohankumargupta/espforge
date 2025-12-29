use crate::parse::EspforgeConfiguration;
use crate::parse::model::Esp32Config;
use crate::parse::processor::{ProcessorRegistration, SectionProcessor};
use anyhow::{Context, Result, bail};
use serde_yaml_ng::Value;
use std::collections::HashMap;

pub struct PlatformProvisioner;

impl SectionProcessor for PlatformProvisioner {
    fn section_key(&self) -> &'static str {
        "esp32"
    }
    fn priority(&self) -> u32 {
        300
    }

    fn process(&self, content: &Value, model: &mut EspforgeConfiguration) -> Result<()> {
        let esp32_config: Esp32Config = serde_yaml_ng::from_value(content.clone())
            .context("Failed to deserialize esp32 configuration")?;

        // Validate pin assignments don't conflict
        self.validate_pin_conflicts(&esp32_config)?;

        model.esp32 = Some(esp32_config);
        println!("✓ ESP32 platform provisioned");
        Ok(())
    }
}

impl PlatformProvisioner {
    fn validate_pin_conflicts(&self, config: &Esp32Config) -> Result<()> {
        let mut used_pins: HashMap<u8, Vec<String>> = HashMap::new();

        // Track GPIO pins
        for (name, gpio_cfg) in &config.gpio {
            used_pins
                .entry(gpio_cfg.pin)
                .or_default()
                .push(format!("gpio.{}", name));
        }

        // Track SPI pins
        for (name, spi_cfg) in &config.spi {
            for (pin, pin_name) in [(spi_cfg.mosi, "mosi"), (spi_cfg.sck, "sck")] {
                used_pins
                    .entry(pin)
                    .or_default()
                    .push(format!("spi.{}.{}", name, pin_name));
            }
            if let Some(miso) = spi_cfg.miso {
                used_pins
                    .entry(miso)
                    .or_default()
                    .push(format!("spi.{}.miso", name));
            }
        }

        // Track I2C pins
        for (name, i2c_cfg) in &config.i2c {
            for (pin, pin_name) in [(i2c_cfg.sda, "sda"), (i2c_cfg.scl, "scl")] {
                used_pins
                    .entry(pin)
                    .or_default()
                    .push(format!("i2c.{}.{}", name, pin_name));
            }
        }

        // Track UART pins
        for (name, uart_cfg) in &config.uart {
            for (pin, pin_name) in [(uart_cfg.tx, "tx"), (uart_cfg.rx, "rx")] {
                used_pins
                    .entry(pin)
                    .or_default()
                    .push(format!("uart.{}.{}", name, pin_name));
            }
        }

        // Find conflicts
        let conflicts: Vec<_> = used_pins
            .iter()
            .filter(|(_, usages)| usages.len() > 1)
            .collect();

        if !conflicts.is_empty() {
            let error_msg = conflicts
                .iter()
                .map(|(pin, usages)| format!("Pin {} used by: {}", pin, usages.join(", ")))
                .collect::<Vec<_>>()
                .join("\n");
            bail!("Pin conflicts detected:\n{}", error_msg);
        }

        Ok(())
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(PlatformProvisioner),
    }
}

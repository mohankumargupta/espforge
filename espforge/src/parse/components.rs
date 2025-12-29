use crate::parse::EspforgeConfiguration;
use crate::parse::model::{Component, ComponentResource, Esp32Config};
use crate::parse::processor::{ProcessorRegistration, SectionProcessor};
use anyhow::{Context, Result, bail};
use serde_yaml_ng::Value;
use std::collections::HashMap;

pub struct ComponentProvisioner;

impl SectionProcessor for ComponentProvisioner {
    fn section_key(&self) -> &'static str {
        "components"
    }
    fn priority(&self) -> u32 {
        200
    } // Process after ESP32

    fn process(&self, content: &Value, model: &mut EspforgeConfiguration) -> Result<()> {
        let components: HashMap<String, Component> = serde_yaml_ng::from_value(content.clone())
            .context("Failed to deserialize components")?;

        // Validate all resource references
        if let Some(esp32) = &model.esp32 {
            self.validate_references(&components, esp32)?;
        } else {
            bail!("Components require esp32 section to be processed first");
        }

        model.components = components;
        println!("✓ {} components provisioned", model.components.len());
        Ok(())
    }
}

impl ComponentProvisioner {
    fn validate_references(
        &self,
        components: &HashMap<String, Component>,
        esp32: &Esp32Config,
    ) -> Result<()> {
        let errors: Vec<_> = components
            .iter()
            .flat_map(|(name, component)| {
                component
                    .resource_refs()
                    .filter_map(|res_ref| {
                        res_ref
                            .reference
                            .strip_prefix('$')
                            .filter(|res_name| {
                                !Self::resource_exists(esp32, res_ref.resource_type, res_name)
                            })
                            .map(|_| {
                                format!(
                                    "Component '{}': {} reference '{}' not found",
                                    name, res_ref.resource_type, res_ref.reference
                                )
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        if !errors.is_empty() {
            bail!("Component validation failed:\n{}", errors.join("\n"));
        }

        Ok(())
    }

    fn resource_exists(esp32: &Esp32Config, res_type: &str, name: &str) -> bool {
        match res_type {
            "gpio" => esp32.gpio.contains_key(name),
            "spi" => esp32.spi.contains_key(name),
            "i2c" => esp32.i2c.contains_key(name),
            "uart" => esp32.uart.contains_key(name),
            _ => false,
        }
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(ComponentProvisioner),
    }
}

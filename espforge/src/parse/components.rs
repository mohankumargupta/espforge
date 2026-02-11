use crate::parse::EspforgeConfiguration;
use crate::parse::processor::SectionProcessor;
use anyhow::{Context, Result, bail};
use espforge_codegen::{dependency::DependencyKind, registry::find_plugin};
use espforge_configuration::{components::Component, hardware::Esp32Config};
use serde_yaml_ng::Value;
use std::collections::HashMap;

pub struct ComponentProvisioner;

impl SectionProcessor for ComponentProvisioner {
    fn section_key(&self) -> &'static str {
        "components"
    }

    fn priority(&self) -> u32 {
        200
    }

    fn process(&self, content: &Value, model: &mut EspforgeConfiguration) -> Result<()> {
        let components: HashMap<String, Component> = serde_yaml_ng::from_value(content.clone())
            .context("Failed to deserialize components")?;

        for _spec in components.values() {
            if let Some(esp32) = &model.esp32 {
                self.validate(&components, esp32)?;
            }
        }

        model.components.extend(components);
        println!("✓ {} components provisioned", model.components.len());
        Ok(())
    }
}

impl ComponentProvisioner {
    fn validate(&self, components: &HashMap<String, Component>, esp32: &Esp32Config) -> Result<()> {
        for (name, spec) in components {
            let plugin = find_plugin(&spec.driver).ok_or_else(|| {
                anyhow::anyhow!("Unknown driver '{}' for component '{}'", spec.driver, name)
            })?;

            // Plugin-specific validation
            plugin
                .validate(&spec.properties)
                .with_context(|| format!("Validation failed for component '{}'", name))?;

            // Resource existence validation
            let deps = plugin.dependencies(&spec.properties)?;
            for dep in deps {
                if dep.kind == DependencyKind::Pin || dep.kind == DependencyKind::Peripheral {
                    let res_name = dep.name.strip_prefix('$').unwrap_or(&dep.name);

                    // Simple existence check for hardware resources
                    if !Self::hardware_exists(esp32, res_name) {
                        bail!(
                            "Component '{}' references unknown hardware resource '{}'",
                            name,
                            res_name
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn hardware_exists(esp32: &Esp32Config, name: &str) -> bool {
        esp32.gpio.contains_key(name)
            || esp32.spi.contains_key(name)
            || esp32.i2c.contains_key(name)
            || esp32.uart.contains_key(name)
    }
}

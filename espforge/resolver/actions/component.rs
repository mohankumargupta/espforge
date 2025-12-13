use crate::config::EspforgeConfiguration;
use crate::manifest::ComponentManifest;
use crate::register_action_strategy;
use crate::resolver::actions::{ActionStrategy, ValidationResult};
use anyhow::{Context, Result, anyhow};
use espforge_macros::auto_register_action_strategy;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use tera::Tera;

#[derive(Default)]
#[auto_register_action_strategy]
pub struct ComponentActionStrategy;

impl ComponentActionStrategy {
    fn parse_key<'a>(&self, key: &'a str) -> Option<(&'a str, &'a str)> {
        // format: $component_name.method_name
        if !key.starts_with('$') {
            return None;
        }

        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() < 2 {
            return None;
        }

        let instance_name = &parts[0][1..]; // strip '$'
        let method_name = parts[1];
        Some((instance_name, method_name))
    }

    fn get_manifest<'a>(
        &self,
        instance_name: &str,
        config: &EspforgeConfiguration,
        manifests: &'a HashMap<String, ComponentManifest>,
    ) -> Result<(&'a ComponentManifest, String)> {
        let components = config
            .components
            .as_ref()
            .ok_or_else(|| anyhow!("No components defined"))?;

        let instance = components
            .get(instance_name)
            .ok_or_else(|| anyhow!("Component instance '{}' not found", instance_name))?;

        let manifest = manifests.get(&instance.using).ok_or_else(|| {
            anyhow!(
                "Manifest '{}' not found for instance '{}'",
                instance.using,
                instance_name
            )
        })?;

        Ok((manifest, instance.using.clone()))
    }
}

impl ActionStrategy for ComponentActionStrategy {
    fn can_handle(&self, key: &str) -> bool {
        key.starts_with('$')
    }

    fn validate(
        &self,
        key: &str,
        _value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
    ) -> ValidationResult {
        let Some((instance_name, method_name)) = self.parse_key(key) else {
            return ValidationResult::Error(format!(
                "Invalid component syntax '{}'. Expected $name.method",
                key
            ));
        };

        match self.get_manifest(instance_name, config, manifests) {
            Ok((manifest, _)) => {
                if manifest.methods.contains_key(method_name) {
                    ValidationResult::Ok(format!(
                        "Validated action '{}' on component '{}'",
                        method_name, instance_name
                    ))
                } else {
                    ValidationResult::Error(format!(
                        "Method '{}' not found on component '{}' (type {})",
                        method_name, instance_name, manifest.name
                    ))
                }
            }
            Err(e) => ValidationResult::Error(e.to_string()),
        }
    }

    fn render(
        &self,
        key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        tera: &mut Tera,
    ) -> Result<String> {
        let (instance_name, method_name) = self
            .parse_key(key)
            .ok_or_else(|| anyhow!("Invalid key format"))?;

        let (manifest, _) = self.get_manifest(instance_name, config, manifests)?;

        let method_def = manifest
            .methods
            .get(method_name)
            .ok_or_else(|| anyhow!("Method not found"))?;

        let mut context = tera::Context::new();
        context.insert("target", instance_name);
        context.insert("args", value);

        tera.render_str(&method_def.template, &context)
            .with_context(|| format!("Failed to render component action {}", key))
    }
}

//register_action_strategy!(ComponentActionStrategy);

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
pub struct GlobalActionStrategy;

impl GlobalActionStrategy {
    fn parse_key<'a>(&self, key: &'a str) -> Option<(&'a str, &'a str)> {
        // format: module.method
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() < 2 {
            return None;
        }
        Some((parts[0], parts[1]))
    }
}

impl ActionStrategy for GlobalActionStrategy {
    fn can_handle(&self, key: &str) -> bool {
        // We handle it if it DOESN'T start with $
        // AND looks like "something.method"
        !key.starts_with('$') && key.contains('.')
    }

    fn validate(
        &self,
        key: &str,
        _value: &Value,
        _config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
    ) -> ValidationResult {
        let Some((module_name, method_name)) = self.parse_key(key) else {
            return ValidationResult::Ignored;
        };

        if let Some(manifest) = manifests.get(module_name) {
            if manifest.methods.contains_key(method_name) {
                ValidationResult::Ok(format!(
                    "Validated global action '{}' on module '{}'",
                    method_name, module_name
                ))
            } else {
                ValidationResult::Error(format!(
                    "Method '{}' not found on global module '{}'",
                    method_name, module_name
                ))
            }
        } else {
            // If we can't find the manifest, this strategy shouldn't definitively error
            // (another strategy might handle it), but since we are the "catch-all" for
            // dot-notation globals, we can issue a warning or error.
            // For now, we assume if it looks like a global call but isn't in manifests, it's invalid.
            ValidationResult::Ignored
        }
    }

    fn render(
        &self,
        key: &str,
        value: &Value,
        _config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        tera: &mut Tera,
    ) -> Result<String> {
        let (module_name, method_name) = self
            .parse_key(key)
            .ok_or_else(|| anyhow!("Invalid key format"))?;

        let manifest = manifests
            .get(module_name)
            .ok_or_else(|| anyhow!("Global module '{}' not found", module_name))?;

        let method_def = manifest.methods.get(method_name).ok_or_else(|| {
            anyhow!(
                "Method '{}' not found in module '{}'",
                method_name,
                module_name
            )
        })?;

        let mut context = tera::Context::new();
        context.insert("target", module_name); // Usually ignored in global templates or used as static ref
        context.insert("args", value);

        tera.render_str(&method_def.template, &context)
            .with_context(|| format!("Failed to render global action {}", key))
    }
}

//register_action_strategy!(GlobalActionStrategy);

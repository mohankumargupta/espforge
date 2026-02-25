use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

const DEPENDENCIES_TOML: &str = include_str!("../../../dependencies.toml");

pub struct ExternalDependencies;

impl ExternalDependencies {
    pub fn merge_embedded(doc: &mut DocumentMut, model: &EspforgeConfiguration) -> Result<()> {
        let deps_template: DocumentMut = DEPENDENCIES_TOML
            .parse()
            .context("Failed to parse embedded dependencies.toml")?;

        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        if let Some(additional_deps) = deps_template.get("dependencies").and_then(|d| d.as_table())
        {
            for (name, value) in additional_deps.iter() {
                if !target_deps.contains_key(name) {
                    target_deps.insert(name, value.clone());
                }
            }
        }

        Self::merge_feature_dependencies(doc, model, &deps_template)?;

        Ok(())
    }

    pub fn merge_external(doc: &mut DocumentMut, config_dir: &Path) -> Result<()> {
        let external_deps_path = config_dir.join("dependencies.toml");

        if !external_deps_path.exists() {
            return Ok(());
        }

        println!("   Merging dependencies from external dependencies.toml...");

        let content = fs::read_to_string(&external_deps_path)
            .context("Failed to read external dependencies.toml")?;

        let ext_doc: DocumentMut = content
            .parse()
            .context("Failed to parse external dependencies.toml")?;

        let ext_deps = ext_doc
            .get("dependencies")
            .and_then(|d| d.as_table())
            .context("Failed to get external dependencies")?;

        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        for (name, value) in ext_deps.iter() {
            if !target_deps.contains_key(name) {
                target_deps.insert(name, value.clone());
            }
        }

        Ok(())
    }

    fn merge_feature_dependencies(
        doc: &mut DocumentMut,
        model: &EspforgeConfiguration,
        deps_template: &DocumentMut,
    ) -> Result<()> {
        let Some(features_table) = deps_template.get("features").and_then(|f| f.as_table()) else {
            return Ok(());
        };

        let Some(esp32) = model.esp32.as_ref() else {
            return Ok(());
        };

        let mut enabled_features = vec![];

        if !esp32.spi.is_empty() {
            enabled_features.push("spi");
        }
        if !esp32.i2c.is_empty() {
            enabled_features.push("i2c");
        }
        if !esp32.uart.is_empty() {
            enabled_features.push("uart");
        }

        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        for used_feature in enabled_features {
            if let Some(enabled_deps) = features_table.get(used_feature).and_then(|v| v.as_array())
            {
                for dep_val in enabled_deps.iter() {
                    if let Some(dep_name) = dep_val.as_str()
                        && !target_deps.contains_key(dep_name)
                        && let Some(template_deps) =
                            deps_template.get("dependencies").and_then(|d| d.as_table())
                        && let Some(dep_value) = template_deps.get(dep_name)
                    {
                        target_deps.insert(dep_name, dep_value.clone());
                    }
                }
            }
        }

        Self::make_dependencies_required(doc, model, deps_template)?;

        Ok(())
    }

    fn make_dependencies_required(
        doc: &mut DocumentMut,
        model: &EspforgeConfiguration,
        deps_template: &DocumentMut,
    ) -> Result<()> {
        let Some(esp32) = model.esp32.as_ref() else {
            return Ok(());
        };

        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        // Collect all dependencies required by enabled features
        let mut required_deps = vec!["embedded-hal".to_string(), "esp-hal".to_string()];

        if let Some(features_table) = deps_template.get("features").and_then(|f| f.as_table()) {
            let mut enabled_features = vec![];

            if !esp32.spi.is_empty() {
                enabled_features.push("spi");
            }
            if !esp32.i2c.is_empty() {
                enabled_features.push("i2c");
            }
            if !esp32.uart.is_empty() {
                enabled_features.push("uart");
            }

            for feature in enabled_features {
                if let Some(feature_deps) = features_table.get(feature).and_then(|v| v.as_array()) {
                    for dep_val in feature_deps.iter() {
                        if let Some(dep_name) = dep_val.as_str() {
                            required_deps.push(dep_name.to_string());
                        }
                    }
                }
            }
        }

        required_deps.sort();
        required_deps.dedup();

        for dep_name in required_deps {
            if let Some(dep_item) = target_deps.get_mut(&dep_name) {
                Self::remove_optional_flag(dep_item);
            }
        }

        Ok(())
    }

    fn remove_optional_flag(dep_item: &mut Item) {
        if let Some(inline_table) = dep_item.as_inline_table_mut() {
            inline_table.remove("optional");
        } else if let Some(table) = dep_item.as_table_like_mut() {
            table.remove("optional");
        }
    }
}

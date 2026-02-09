use anyhow::{Context, Result};
use espforge_codegen::registry;
use espforge_configuration::EspforgeConfiguration;
use espforge_configuration::plugin::PluginKind;
use std::fs;
use std::path::Path;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Value};

const PLATFORM_VERSION: &str = env!("ESPFORGE_PLATFORM_VERSION");
const DEVICES_VERSION: &str = env!("ESPFORGE_DEVICES_VERSION");
const COMPONENTS_VERSION: &str = env!("ESPFORGE_COMPONENTS_VERSION");
const ESPFORGE_REPO: &str = "https://github.com/mohankumargupta/espforge";
const DEPENDENCIES_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/dependencies.toml"));

pub fn add_dependencies(project_dir: &Path, model: &EspforgeConfiguration) -> Result<()> {
    let cargo_path = project_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;
    let mut doc = manifest
        .parse::<DocumentMut>()
        .context("Failed to parse Cargo.toml")?;

    if let Some(target_deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
        let create_dep = |version: &str| -> Item {
            let mut dep = InlineTable::new();
            let use_git = std::env::var("ESPFORGE_USE_GIT").is_ok();
            if !use_git {
                dep.get_or_insert("version", version);
            } else {
                dep.get_or_insert("git", ESPFORGE_REPO);
                dep.get_or_insert("branch", "dev");
            }
            toml_edit::value(dep)
        };

        let mut platform_dep = create_dep(PLATFORM_VERSION);
        let mut components_dep = create_dep(COMPONENTS_VERSION);

        // Collect platform features needed based on hardware configuration
        let mut platform_features = vec![];

        // Check which hardware peripherals are actually used
        if let Some(esp32) = &model.esp32 {
            if !esp32.spi.is_empty() {
                platform_features.push("spi".to_string());
            }
            if !esp32.i2c.is_empty() {
                platform_features.push("i2c".to_string());
            }
            if !esp32.uart.is_empty() {
                platform_features.push("uart".to_string());
            }
        }

        if model.is_embassy() {
            let add_embassy_feature = |dep_item: &mut Item| {
                if let Some(table) = dep_item.as_inline_table_mut() {
                    let mut features = Array::new();
                    features.push("embassy");
                    table.insert("features", Value::Array(features));
                }
            };

            //add_embassy_feature(&mut platform_dep);
            add_embassy_feature(&mut components_dep);
            platform_features.push("embassy".to_string());
        }

        // Collect features from plugins
        let mut device_features = Vec::new();
        let mut component_features = Vec::new();

        for spec in model.devices.values() {
            if let Some(plugin) = registry::find_plugin(&spec.driver) {
                if plugin.kind() == PluginKind::Device {
                    device_features.extend(plugin.required_features());
                }
            }
        }

        for spec in model.components.values() {
            if let Some(plugin) = registry::find_plugin(&spec.driver) {
                if plugin.kind() == PluginKind::Component {
                    component_features.extend(plugin.required_features());
                }
            }
        }

        device_features.sort();
        device_features.dedup();
        component_features.sort();
        component_features.dedup();
        add_features(&mut components_dep, component_features);
        let mut devices_dep = create_dep(DEVICES_VERSION);
        add_features(&mut devices_dep, device_features);
        add_features(&mut platform_dep, platform_features);
        target_deps.insert("espforge_platform", platform_dep);
        target_deps.insert("espforge_components", components_dep);
        if !model.devices.is_empty() {
            target_deps.insert("espforge_devices", devices_dep);
        }

        // Add dependencies from dependencies.toml
        let deps_template: DocumentMut = DEPENDENCIES_TOML
            .parse()
            .context("Failed to parse embedded dependencies.toml")?;

        if let Some(additional_deps) = deps_template.get("dependencies").and_then(|d| d.as_table())
        {
            for (name, value) in additional_deps.iter() {
                if !target_deps.contains_key(name) {
                    target_deps.insert(name, value.clone());
                }
            }
        }

        let mut required_features = std::collections::HashSet::new();
        if let Some(esp32) = &model.esp32 {
            if !esp32.spi.is_empty() {
                required_features.insert("spi");
            }
            if !esp32.i2c.is_empty() {
                required_features.insert("i2c");
            }
            if !esp32.uart.is_empty() {
                required_features.insert("uart");
            }
        }

        for feature in &required_features {
            if let Some(feature_deps) = deps_template
                .get("features")
                .and_then(|f| f.as_table())
                .and_then(|t| t.get(*feature))
                .and_then(|v| v.as_array())
            {
                for dep_name in feature_deps.iter().filter_map(|v| v.as_str()) {
                    if let Some(dep_config) = deps_template
                        .get("dependencies")
                        .and_then(|d| d.as_table())
                        .and_then(|t| t.get(dep_name))
                        .and_then(|d| d.as_table())
                    {
                        let mut inline = toml_edit::InlineTable::new();
                        for (k, v) in dep_config.iter() {
                            if k == "optional" {
                                continue;
                            }
                            if let Some(value) = v.as_value() {
                                inline.insert(k, value.clone());
                            }
                        }
                        target_deps.insert(
                            dep_name,
                            toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)),
                        );
                    }
                }
            }
        }
    }

    fs::write(cargo_path, doc.to_string()).context("Failed to write Cargo.toml")?;
    Ok(())
}

fn add_features(dep_item: &mut Item, features_list: Vec<String>) {
    if features_list.is_empty() {
        return;
    }
    if let Some(table) = dep_item.as_inline_table_mut() {
        let existing_features = table
            .entry("features")
            .or_insert(Value::Array(toml_edit::Array::new()));
        if let Some(arr) = existing_features.as_array_mut() {
            for f in features_list {
                arr.push(f);
            }
        }
    }
}

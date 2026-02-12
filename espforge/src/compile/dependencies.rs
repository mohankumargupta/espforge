use anyhow::{Context, Result};
use espforge_codegen::registry;
use espforge_configuration::EspforgeConfiguration;
use espforge_configuration::plugin::PluginKind;
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value};

const ESPFORGE_REPO: &str = "https://github.com/mohankumargupta/espforge";
const DEPENDENCIES_TOML: &str = include_str!("../../dependencies.toml");
const VERSIONS_TOML: &str = include_str!("../../espforge_versions.toml");

pub fn add_dependencies(project_dir: &Path, model: &EspforgeConfiguration) -> Result<()> {
    let cargo_path = project_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;

    let mut doc: DocumentMut = manifest.parse().context("Failed to parse Cargo.toml")?;

    // Parse versions from the embedded versions TOML
    let versions_doc: DocumentMut = VERSIONS_TOML
        .parse()
        .context("Failed to parse embedded espforge_versions.toml")?;

    let get_ver = |key: &str| -> Result<&str> {
        versions_doc
            .get("espforge")
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing version for {}", key))
    };

    let platform_version = get_ver("espforge_platform")?;
    let components_version = get_ver("espforge_components")?;
    let devices_version = get_ver("espforge_devices")?;

    if let Some(target_deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
        let use_git = std::env::var("ESPFORGE_USE_GIT").is_ok();

        let create_dep = |version: &str| -> Item {
            let mut dep = toml_edit::InlineTable::new();
            if use_git {
                dep.get_or_insert("git", ESPFORGE_REPO);
                dep.get_or_insert("branch", "dev");
            } else {
                dep.get_or_insert("version", version);
            }
            Item::Value(Value::InlineTable(dep))
        };

        let mut platform_dep = create_dep(platform_version);
        let mut components_dep = create_dep(components_version);
        let mut platform_features = vec![];

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
            if let Some(table) = components_dep.as_inline_table_mut() {
                let mut features = toml_edit::Array::new();
                features.push("embassy");
                table.insert("features", Value::Array(features));
            }
            platform_features.push("embassy".to_string());
        }

        let mut device_features = vec![];
        for spec in model.devices.values() {
            if let Some(plugin) = registry::find_plugin(&spec.driver) {
                if plugin.kind() == PluginKind::Device {
                    device_features.extend(plugin.required_features());
                }
            }
        }

        let mut component_features = vec![];
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
        platform_features.sort();
        platform_features.dedup();

        add_features(&mut components_dep, component_features);

        let mut devices_dep = create_dep(devices_version);
        add_features(&mut devices_dep, device_features);
        add_features(&mut platform_dep, platform_features.clone());

        target_deps.insert("espforge_platform", platform_dep);
        target_deps.insert("espforge_components", components_dep);

        if !model.devices.is_empty() {
            target_deps.insert("espforge_devices", devices_dep);
        }

        // Apply external dependencies from dependencies.toml
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

        if let Some(features_table) = deps_template.get("features").and_then(|f| f.as_table()) {
            for used_feature in &platform_features {
                if let Some(enabled_deps) =
                    features_table.get(used_feature).and_then(|v| v.as_array())
                {
                    for dep_val in enabled_deps.iter() {
                        if let Some(dep_name) = dep_val.as_str() {
                            // Ensure the dependency exists in target_deps
                            if !target_deps.contains_key(dep_name) {
                                // If it's in the template dependencies, add it
                                if let Some(template_deps) =
                                    deps_template.get("dependencies").and_then(|d| d.as_table())
                                {
                                    if let Some(dep_value) = template_deps.get(dep_name) {
                                        target_deps.insert(dep_name, dep_value.clone());
                                    }
                                }
                            }
                            // Now remove the optional flag
                            if let Some(dep_item) = target_deps.get_mut(dep_name) {
                                if let Some(inline_table) = dep_item.as_inline_table_mut() {
                                    inline_table.remove("optional");
                                }

                                else if let Some(table) = dep_item.as_table_like_mut() {
                                    table.remove("optional");
                                }
                            }
                        }
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
                let s = Value::from(f);
                arr.push(s);
            }
        }
    }
}

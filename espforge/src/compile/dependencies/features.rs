use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use espforge_configuration::plugin::PluginKind;
use toml_edit::{DocumentMut, Item, Value};

pub struct FeatureManager;

impl FeatureManager {
    pub fn add_platform_features(
        doc: &mut DocumentMut,
        model: &EspforgeConfiguration,
    ) -> Result<()> {
        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        let Some(esp32) = model.esp32.as_ref() else {
            return Ok(());
        };

        let mut platform_features = vec![];

        if !esp32.spi.is_empty() {
            platform_features.push("spi".to_string());
        }

        if !esp32.i2c.is_empty() {
            platform_features.push("i2c".to_string());
        }

        if !esp32.uart.is_empty() {
            platform_features.push("uart".to_string());
        }

        if esp32.psram.is_some() {
            platform_features.push("psram".to_string());
        }

        if model.is_embassy() {
            platform_features.push("embassy".to_string());
        }

        if esp32.wifi.is_some() {
            platform_features.push("wifi".to_string());
        }

        if let Some(platform_dep) = target_deps.get_mut("espforge_platform") {
            Self::add_features(platform_dep, platform_features);
        }

        Ok(())
    }

    pub fn add_component_features(
        doc: &mut DocumentMut,
        model: &EspforgeConfiguration,
    ) -> Result<()> {
        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        let mut component_features = vec![];

        for spec in model.components.values() {
            if let Some(plugin) = espforge_codegen::registry::find_plugin(&spec.driver)
                && plugin.kind() == PluginKind::Component
            {
                component_features.extend(plugin.required_features());
            }
        }

        component_features.sort();
        component_features.dedup();

        if let Some(components_dep) = target_deps.get_mut("espforge_components") {
            Self::add_features(components_dep, component_features);
        }

        Ok(())
    }

    pub fn add_device_features(doc: &mut DocumentMut, model: &EspforgeConfiguration) -> Result<()> {
        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        if model.devices.is_empty() {
            return Ok(());
        }

        let mut device_features = vec![];

        for spec in model.devices.values() {
            if let Some(plugin) = espforge_codegen::registry::find_plugin(&spec.driver)
                && plugin.kind() == PluginKind::Device
            {
                device_features.extend(plugin.required_features());
            }
        }

        device_features.sort();
        device_features.dedup();

        if let Some(devices_dep) = target_deps.get_mut("espforge_devices") {
            Self::add_features(devices_dep, device_features);
        }

        Ok(())
    }

    pub fn handle_embassy_features(
        doc: &mut DocumentMut,
        model: &EspforgeConfiguration,
    ) -> Result<()> {
        let target_deps = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        if !model.is_embassy() {
            return Ok(());
        }

        let mut features = vec!["embassy".to_string()];

        if let Some(components_dep) = target_deps.get_mut("espforge_components")
            && let Some(table) = components_dep.as_inline_table_mut()
        {
            if let Some(existing) = table.get("features")
                && let Some(arr) = existing.as_array()
            {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        features.push(s.to_string());
                    }
                }
            }
            table.insert(
                "features",
                Value::Array(toml_edit::Array::from_iter(features)),
            );
        }

        if let Some(platform_dep) = target_deps.get_mut("espforge_platform") {
            Self::add_features(platform_dep, vec!["embassy".to_string()]);
        }

        Ok(())
    }

    pub fn handle_psram(doc: &mut DocumentMut, model: &EspforgeConfiguration) -> Result<()> {
        if model
            .esp32
            .as_ref()
            .and_then(|e| e.psram.as_ref())
            .is_none()
        {
            return Ok(());
        }

        println!("   Detected PSRAM configuration - adding 'psram' feature to esp-hal");

        let deps_table = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .context("Failed to get dependencies table")?;

        if let Some(esp_hal_item) = deps_table.get_mut("esp-hal") {
            Self::add_psram_feature(esp_hal_item);
        }

        Ok(())
    }

    fn add_psram_feature(esp_hal_item: &mut Item) {
        if let Some(inline_table) = esp_hal_item.as_inline_table_mut() {
            if let Some(features_value) = inline_table.get_mut("features")
                && let Some(features_array) = features_value.as_array_mut()
            {
                let has_psram = features_array.iter().any(|v| v.as_str() == Some("psram"));

                if !has_psram {
                    features_array.push("psram");
                }
            }
        } else if let Some(table) = esp_hal_item.as_table_mut()
            && let Some(features_item) = table.get_mut("features")
            && let Some(features_value) = features_item.as_value_mut()
            && let Some(arr) = features_value.as_array_mut()
        {
            let has_psram = arr.iter().any(|v| v.as_str() == Some("psram"));

            if !has_psram {
                arr.push("psram");
            }
        }
    }

    // pub fn add_wifi_features(doc: &mut DocumentMut, model: &EspforgeConfiguration) -> Result<()> {
    //     if model.esp32.as_ref().and_then(|e| e.wifi.as_ref()).is_none() {
    //         return Ok(());
    //     }
    //     let target_deps = doc
    //         .get_mut("dependencies")
    //         .and_then(|d| d.as_table_mut())
    //         .context("Failed to get dependencies")?;

    //     // Make wifi deps non-optional
    //     for dep in &["embassy-net", "esp-radio"] {
    //         if let Some(item) = target_deps.get_mut(*dep) {
    //                 if let Some(inline_table) = item.as_inline_table_mut() {
    //     inline_table.remove("optional");
    // } else if let Some(table) = item.as_table_like_mut() {
    //     table.remove("optional");
    // }
    //         }
    //     }
    //     Ok(())
    // }

    fn add_features(dep_item: &mut Item, features_list: Vec<String>) {
        if features_list.is_empty() {
            return;
        }

        if let Some(table) = dep_item.as_inline_table_mut() {
            let existing_features = table
                .entry("features")
                .or_insert(Value::Array(toml_edit::Array::new()));

            if let Some(arr) = existing_features.as_array_mut() {
                for s in features_list {
                    arr.push(s);
                }
            }
        } else if let Some(table) = dep_item.as_table_mut() {
            let existing_features =
                table
                    .entry("features")
                    .or_insert(toml_edit::Item::Value(
                        Value::Array(toml_edit::Array::new()),
                    ));

            if let Some(arr) = existing_features.as_array_mut() {
                for s in features_list {
                    arr.push(s);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_features_empty() {
        let mut item = Item::from(toml_edit::InlineTable::new());
        FeatureManager::add_features(&mut item, vec![]);
    }

    #[test]
    fn test_add_features_inline_table() {
        let table = toml_edit::InlineTable::new();
        let item = Item::from(table);
        FeatureManager::add_features(&mut item.clone(), vec!["feature1".to_string()]);
    }
}

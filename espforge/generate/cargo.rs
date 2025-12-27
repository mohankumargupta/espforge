use super::manifest;
use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::fs;
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item, Table, Value};

const CARGO_TEMPLATE_NAME: &str = "Cargo.toml.tera";

pub fn update_manifest(
    project_path: &Path, 
    config_dir: &Path, 
    chip: &str,
    context: &tera::Context
) -> Result<()> {
    let cargo_path = project_path.join("Cargo.toml");
    let cargo_content = fs::read_to_string(&cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path.display()))?;
    
    let mut doc = cargo_content.parse::<DocumentMut>()
        .context("Failed to parse Cargo.toml")?;

    // 1. Merge dependencies from _dynamic/Cargo.toml.tera (Embedded)
    if let Some(dynamic_file) = crate::template_utils::get_templates()
        .get_file("_dynamic/Cargo.toml.tera") 
    {
        let content = dynamic_file.contents_utf8()
            .context("_dynamic/Cargo.toml.tera is invalid UTF-8")?;
        let rendered = tera::Tera::one_off(content, context, true)
            .context("Failed to render _dynamic/Cargo.toml.tera")?;
        let dynamic_doc = rendered.parse::<DocumentMut>()
            .context("Failed to parse rendered _dynamic cargo template")?;
        
        merge_documents(&mut doc, &dynamic_doc);
    }

    // 2. Merge dependencies from user's config_dir/Cargo.toml.tera (Filesystem)
    let user_tera = config_dir.join("Cargo.toml.tera");
    if user_tera.exists() {
        let content = fs::read_to_string(&user_tera)?;
        let rendered = tera::Tera::one_off(&content, context, true)
            .context("Failed to render user Cargo.toml.tera")?;
        let user_doc = rendered.parse::<DocumentMut>()
            .context("Failed to parse user Cargo.toml.tera")?;
        
        merge_documents(&mut doc, &user_doc);
    }

    // 3. System updates
    add_chip_features(&mut doc, chip)?;
    ensure_workspace_exists(&mut doc);
    merge_device_dependencies(&mut doc)?;

    fs::write(&cargo_path, doc.to_string())
        .with_context(|| format!("Failed to write {}", cargo_path.display()))?;

    info!("Updated Cargo.toml");
    Ok(())
}

fn merge_documents(target: &mut DocumentMut, source: &DocumentMut) {
    // FIX: Added "features" to the list so it gets copied from the template
    for section in ["dependencies", "build-dependencies", "dev-dependencies", "features"] {
        if let Some(source_table) = source.get(section).and_then(|i| i.as_table()) {
            if !target.contains_key(section) {
                target[section] = Item::Table(Table::new());
            }
            let target_table = target[section].as_table_mut().unwrap();
            
            for (k, v) in source_table.iter() {
                // Don't overwrite existing keys to prefer esp-generate defaults or previous merges
                if !target_table.contains_key(k) {
                    target_table.insert(k, v.clone());
                }
            }
        }
    }
}

fn add_chip_features(doc: &mut DocumentMut, chip: &str) -> Result<()> {
    if !doc.contains_key("features") {
        doc["features"] = Item::Table(Table::new());
    }
    
    let features = doc["features"]
        .as_table_mut()
        .context("'features' is not a table")?;

    // let mut default_array = Array::new();
    // default_array.push(chip);
    // features["default"] = Item::Value(Value::Array(default_array));
    
    let default_item = features.entry("default")
        .or_insert(Item::Value(Value::Array(Array::new())));

    let default_array = default_item
        .as_value_mut()
        .context("'default' feature is not a value")?
        .as_array_mut()
        .context("'default' feature is not an array")?;

    // Add chip to default if not present
    let exists = default_array.iter().any(|v| v.as_str() == Some(chip));
    if !exists {
        default_array.push(chip);
    }

    if !features.contains_key(chip) {
        features[chip] = Item::Value(Value::Array(Array::new()));
    }
    
    debug!("Added feature for chip: {}", chip);
    Ok(())
}

fn ensure_workspace_exists(doc: &mut DocumentMut) {
    if !doc.contains_key("workspace") {
        doc["workspace"] = Item::Table(Table::new());
    }
}

fn merge_device_dependencies(doc: &mut DocumentMut) -> Result<()> {
    if !doc.contains_key("dependencies") {
        doc["dependencies"] = Item::Table(Table::new());
    }
    
    let root_deps = doc["dependencies"]
        .as_table_mut()
        .context("'dependencies' is not a table")?;

    for subdir in manifest::devices_dir().dirs() {
        if let Some(cargo_file) = find_cargo_template(subdir) {
            let device_toml_str = cargo_file
                .contents_utf8()
                .context("Device Cargo.toml.tera is not valid UTF-8")?;

            let device_doc = device_toml_str.parse::<DocumentMut>()
                .with_context(|| {
                    format!("Failed to parse Cargo.toml.tera for device {:?}", subdir.path())
                })?;

            if let Some(deps) = device_doc.get("dependencies").and_then(|d| d.as_table()) {
                for (key, value) in deps.iter() {
                    if root_deps.contains_key(key) {
                        warn!("Dependency '{}' already exists, skipping from device", key);
                    } else {
                        root_deps.insert(key, value.clone());
                        debug!("Added dependency: {}", key);
                    }
                }
            }
        }
    }
    
    Ok(())
}

fn find_cargo_template<'a>(subdir: &'a include_dir::Dir<'a>) -> Option<&'a include_dir::File<'a>> {
    subdir.files()
        .find(|f| f.path().file_name().and_then(|n| n.to_str()) == Some(CARGO_TEMPLATE_NAME))
}


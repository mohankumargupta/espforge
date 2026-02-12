use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Value};

pub fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=dependencies.toml");
    println!("cargo:rerun-if-changed=espforge_versions.toml");
    println!("cargo:rerun-if-changed=../Cargo.toml");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);

    let workspace_root = match manifest_dir.parent() {
        Some(p) if p.join("Cargo.toml").exists() => p,
        _ => return Ok(()),
    };

    update_external_dependencies(&manifest_dir, workspace_root)?;
    update_internal_versions(&manifest_dir, workspace_root)?;

    Ok(())
}

fn update_external_dependencies(manifest_dir: &Path, workspace_root: &Path) -> Result<()> {
    let deps_path = manifest_dir.join("dependencies.toml");
    let workspace_toml = workspace_root.join("Cargo.toml");

    let workspace_versions = read_workspace_versions(&workspace_toml)?;
    if workspace_versions.is_empty() {
        return Ok(());
    }

    let content = fs::read_to_string(&deps_path).context("Failed to read dependencies.toml")?;
    let mut doc: DocumentMut = content
        .parse()
        .context("Failed to parse dependencies.toml")?;
    let mut modified = false;

    if let Some(deps_table) = doc.get_mut("dependencies").and_then(|d| d.as_table_mut()) {
        for (dep_name, dep_item) in deps_table.iter_mut() {
            let name = dep_name.get();
            if let Some(new_version) = workspace_versions.get(name) {
                if update_item_version(dep_item, new_version) {
                    modified = true;
                }
            }
        }
    }

    if modified {
        fs::write(&deps_path, doc.to_string()).context("Failed to update dependencies.toml")?;
    }
    Ok(())
}

fn update_internal_versions(manifest_dir: &Path, workspace_root: &Path) -> Result<()> {
    let versions_path = manifest_dir.join("espforge_versions.toml");

    let content =
        fs::read_to_string(&versions_path).context("Failed to read espforge_versions.toml")?;
    let mut doc: DocumentMut = content
        .parse()
        .context("Failed to parse espforge_versions.toml")?;
    let mut modified = false;

    let internal_crates = [
        "espforge_platform",
        "espforge_components",
        "espforge_devices",
    ];

    if let Some(table) = doc.get_mut("espforge").and_then(|t| t.as_table_mut()) {
        for crate_name in internal_crates {
            if let Ok(version) = get_crate_version(workspace_root, crate_name) {
                if let Some(item) = table.get_mut(crate_name) {
                    if let Some(val) = item.as_value_mut() {
                        if val.as_str() != Some(&version) {
                            *val = Value::from(version);
                            modified = true;
                        }
                    }
                }
            }
        }
    }

    if modified {
        fs::write(&versions_path, doc.to_string())
            .context("Failed to update espforge_versions.toml")?;
    }
    Ok(())
}

fn read_workspace_versions(path: &Path) -> Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let doc: DocumentMut = content.parse()?;
    let mut versions = HashMap::new();

    if let Some(deps) = doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        for (key, value) in deps.iter() {
            let version = if let Some(v) = value.get("version").and_then(|v| v.as_str()) {
                v.to_string()
            } else if let Some(v) = value.as_str() {
                v.to_string()
            } else {
                continue;
            };
            versions.insert(key.to_string(), version);
        }
    }
    Ok(versions)
}

fn get_crate_version(root: &Path, name: &str) -> Result<String> {
    let cargo_path = root.join(name).join("Cargo.toml");
    let content = fs::read_to_string(&cargo_path)?;
    let doc: DocumentMut = content.parse()?;

    doc.get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Version not found in {}", cargo_path.display()))
}

fn update_item_version(item: &mut Item, new_version: &str) -> bool {
    if let Some(val) = item.as_value_mut() {
        if val.as_str() != Some(new_version) {
            *val = Value::from(new_version);
            return true;
        }
    } else if let Some(table) = item.as_inline_table_mut() {
        if let Some(val) = table.get_mut("version") {
            if val.as_str() != Some(new_version) {
                *val = Value::from(new_version);
                return true;
            }
        }
    } else if let Some(table) = item.as_table_mut() {
        if let Some(item_ver) = table.get_mut("version") {
            if let Some(val) = item_ver.as_value_mut() {
                if val.as_str() != Some(new_version) {
                    *val = Value::from(new_version);
                    return true;
                }
            }
        }
    }
    false
}

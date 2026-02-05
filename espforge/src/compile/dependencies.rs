use anyhow::{anyhow, Context, Result};
use espforge_configuration::EspforgeConfiguration;
use espforge_examples::EXTRA_DEPENDENCIES;
use std::fs;
use std::path::Path;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Value};

const PLATFORM_VERSION: &str = env!("ESPFORGE_PLATFORM_VERSION");
const DEVICES_VERSION: &str = env!("ESPFORGE_DEVICES_VERSION");
const COMPONENTS_VERSION: &str = env!("ESPFORGE_COMPONENTS_VERSION");
const ESPFORGE_REPO: &str = "https://github.com/mohankumargupta/espforge";

pub fn add_dependencies(project_dir: &Path, model: &EspforgeConfiguration) -> Result<()> {
    let cargo_path = project_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;
    let mut doc = manifest
        .parse::<DocumentMut>()
        .context("Failed to parse Cargo.toml")?;

    let deps_doc = EXTRA_DEPENDENCIES
        .parse::<DocumentMut>()
        .context("Failed to parse reference dependencies.toml")?;

    let extra_deps = deps_doc
        .get("dependencies")
        .and_then(|item| item.as_table())
        .ok_or_else(|| anyhow!("dependencies.toml is missing the [dependencies] section"))?;

    if let Some(target_deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
        for (key, value) in extra_deps.iter() {
            if !target_deps.contains_key(key) {
                target_deps.insert(key, value.clone());
            }
        }

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

        if model.is_embassy() {
            let add_embassy_feature = |dep_item: &mut Item| {
                if let Some(table) = dep_item.as_inline_table_mut() {
                    let mut features = Array::new();
                    features.push("embassy");
                    table.insert("features", Value::Array(features));
                }
            };

            add_embassy_feature(&mut platform_dep);
            add_embassy_feature(&mut components_dep);
        }

        target_deps.insert("espforge_platform", platform_dep);
        target_deps.insert("espforge_components", components_dep);
        target_deps.insert("espforge_devices", create_dep(DEVICES_VERSION));
    }

    fs::write(cargo_path, doc.to_string()).context("Failed to write Cargo.toml")?;
    Ok(())
}

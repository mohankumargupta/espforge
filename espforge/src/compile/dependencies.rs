use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use espforge_examples::EXTRA_DEPENDENCIES;

pub fn add_dependencies(project_dir: &Path) -> Result<()> {
    let cargo_path = project_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;
    let mut doc = manifest.parse::<toml_edit::DocumentMut>().context("Failed to parse Cargo.toml")?;

    // Parse the reference dependencies from the examples crate
    let deps_doc = EXTRA_DEPENDENCIES.parse::<toml_edit::DocumentMut>()
        .context("Failed to parse reference dependencies.toml")?;

    let extra_deps = deps_doc.get("dependencies")
        .and_then(|item| item.as_table())
        .ok_or_else(|| anyhow!("dependencies.toml is missing the [dependencies] section"))?;

    if let Some(target_deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
        // Copy standard dependencies (embedded-hal, etc)
        for (key, value) in extra_deps.iter() {
            if !target_deps.contains_key(key) {
                target_deps.insert(key, value.clone());
            }
        }

        // Add espforge_platform as a Git dependency
        let mut platform_dep = toml_edit::InlineTable::new();
        platform_dep.get_or_insert("git", "https://github.com/mohankumargupta/espforge");
        platform_dep.get_or_insert("branch", "dev");
        target_deps.insert("espforge_platform", toml_edit::value(platform_dep));

        // Add espforge_devices as a Git dependency
        let mut devices_dep = toml_edit::InlineTable::new();
        devices_dep.get_or_insert("git", "https://github.com/mohankumargupta/espforge");
        devices_dep.get_or_insert("branch", "dev");
        target_deps.insert("espforge_devices", toml_edit::value(devices_dep));
    }

    fs::write(cargo_path, doc.to_string()).context("Failed to write Cargo.toml")?;
    Ok(())
}
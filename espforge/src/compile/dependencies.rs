use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;

/// Merges required dependencies from espforge_examples into the generated Cargo.toml
pub fn add_dependencies(project_dir: &Path) -> Result<()> {
    let cargo_path = project_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;
    let mut doc = manifest.parse::<toml_edit::DocumentMut>().context("Failed to parse Cargo.toml")?;
    
    // Parse the embedded dependencies file
    let reference_toml = espforge_examples::EXTRA_DEPENDENCIES
        .parse::<toml_edit::DocumentMut>()
        .context("Failed to parse reference dependencies.toml")?;

    // Extract the [dependencies] table from the reference
    let source_deps = reference_toml
        .get("dependencies")
        .and_then(|item| item.as_table())
        .ok_or_else(|| anyhow!("dependencies.toml is missing the [dependencies] section"))?;

    // Merge into the target [dependencies]
    if let Some(target_deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
        for (dep_name, dep_value) in source_deps {
            // Only add if not already present (prevents overwriting user customizations if they exist)
            if !target_deps.contains_key(dep_name) {
                target_deps[dep_name] = dep_value.clone();
            }
        }
    }        
    
    fs::write(cargo_path, doc.to_string()).context("Failed to write Cargo.toml")?;
    Ok(())
}

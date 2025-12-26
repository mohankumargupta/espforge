use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn refine_project_files(project_path: &Path, config_dir: &Path, context: &tera::Context) -> Result<()> {
    merge_cargo_dependencies(project_path, config_dir, context)?;
    apply_local_overrides(project_path, config_dir, context)?;
    Ok(())
}

fn merge_cargo_dependencies(project_path: &Path, config_dir: &Path, context: &tera::Context) -> Result<()> {
    let cargo_path = project_path.join("Cargo.toml");
    let current_toml = fs::read_to_string(&cargo_path)?;
    let mut manifest: toml::Table = toml::from_str(&current_toml)?;

    let local_tera = config_dir.join("Cargo.toml.tera");
    if local_tera.exists() {
        let rendered = tera::Tera::one_off(&fs::read_to_string(local_tera)?, context, true)
            .context("Failed to render local Cargo.toml.tera")?;
        
        let source_manifest: toml::Table = toml::from_str(&rendered)?;
        
        for section in ["dependencies", "build-dependencies"] {
            if let Some(source_table) = source_manifest.get(section).and_then(|v| v.as_table()) {
                let target_table = manifest.entry(section.to_string())
                    .or_insert(toml::Value::Table(toml::Table::new()))
                    .as_table_mut().unwrap();
                for (k, v) in source_table { target_table.insert(k.clone(), v.clone()); }
            }
        }
    }

    fs::write(&cargo_path, toml::to_string_pretty(&manifest)?)?;
    Ok(())
}

fn apply_local_overrides(project_path: &Path, config_dir: &Path, context: &tera::Context) -> Result<()> {
    // Rendered files
    if let Some(rendered_pair) = [("wokwi.toml.tera", "wokwi.toml")]
        .iter()
        .find(|(src, _)| config_dir.join(src).exists()) 
    {
        let content = fs::read_to_string(config_dir.join(rendered_pair.0))?;
        let rendered = tera::Tera::one_off(&content, context, true)?;
        fs::write(project_path.join(rendered_pair.1), rendered)?;
    }

    // Static assets
    let assets = ["diagram.json", "wokwi.toml", "chip.wasm", "chip.json"];
    for asset in assets {
        let src = config_dir.join(asset);
        if src.exists() {
            fs::copy(&src, project_path.join(asset))?;
        }
    }
    Ok(())
}

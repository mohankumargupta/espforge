use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn refine_project_files(project_path: &Path, config_dir: &Path, context: &tera::Context) -> Result<()> {
    // Cargo dependencies are now handled in generate::cargo::update_manifest
    apply_local_overrides(project_path, config_dir, context)?;
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
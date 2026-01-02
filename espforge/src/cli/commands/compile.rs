use std::fs;
use std::path::Path;

use crate::{codegen::espgenerate::esp_generate, parse::EspforgeConfiguration};
use anyhow::{Context, Result};
use toml_edit::DocumentMut;

pub fn execute(file: &Path) -> Result<()> {
    let content = fs::read_to_string(file).context(format!(
        "Failed to read configuration file: {}",
        file.display()
    ))?;

    let config: EspforgeConfiguration =
        serde_yaml_ng::from_str(&content).context("Failed to parse configuration")?;

    println!("Configuration valid for project: {}", config.get_name());

    esp_generate(config.get_name(), &config.get_chip(), false)?;

    let base_dir = file.parent().unwrap_or_else(|| Path::new("."));
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;
    let project_dir = current_dir.join(config.get_name());
    let src_dir = project_dir.join("src");

    // 1. Prepare Platform Assets in src/platform
    let platform_temp = project_dir.join(".espforge_temp");
    fs::create_dir_all(&platform_temp)?;
    crate::PLATFORM_SRC.extract(&platform_temp).context("Failed to extract platform")?;

    let assets_dir = platform_temp.join("assets");
    let platform_dest = src_dir.join("platform");
    
    if assets_dir.exists() {
        // Copy assets to src/platform, renaming lib.rs to mod.rs
        copy_platform_assets(&assets_dir, &platform_dest)?;
    }
    let _ = fs::remove_dir_all(platform_temp);

    // 2. Setup src/lib.rs to tie app and platform together
    let lib_rs_content = r#"#![no_std]
pub mod app;
pub mod platform;
// Wildcard export platform items so they are easily accessible
pub use platform::*;
"#;
    fs::write(src_dir.join("lib.rs"), lib_rs_content)
        .context("Failed to write src/lib.rs")?;


    // 3. Copy user app code to src/app.rs
    let app_rust_src = base_dir.join("app/rust/app.rs");
    if app_rust_src.exists() {
        fs::copy(&app_rust_src, src_dir.join("app.rs"))
            .context("Failed to copy app.rs to src/app.rs")?;
        println!("Copied app logic to src/app.rs");
    } else {
        println!("Warning: No app/rust/app.rs found.");
    }

    // 4. Update Cargo.toml (cleanup only, no dependency injection needed for platform)
    // We read it just to ensure it exists and is valid, or if we needed to make other changes.
    // The previous logic inserting 'espforge_platform' is removed.
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let cargo_toml_content = fs::read_to_string(&cargo_toml_path)?;
    let _manifest = cargo_toml_content.parse::<DocumentMut>()?;

    // 5. Generate main.rs
    // Rust crates use underscores instead of hyphens in code
    let crate_name = config.get_name().replace('-', "_");
    let main_rs_content = espforge_templates::render_main(None, &crate_name, "", "")
        .map_err(|e| anyhow::anyhow!("Failed to render template: {}", e))?;

    let main_rs_path = src_dir.join("bin/main.rs");
    if let Some(parent) = main_rs_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    fs::write(&main_rs_path, main_rs_content)
        .context("Failed to write generated main.rs")?;
    println!("✨ wired up main.rs");

    Ok(())
}

fn copy_platform_assets(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target_path = dst.join(entry.file_name());
        
        if file_type.is_dir() {
            copy_platform_assets(&entry.path(), &target_path)?;
        } else {
            if entry.file_name() == "lib.rs" {
                fs::copy(entry.path(), dst.join("mod.rs"))?;
            } else {
                fs::copy(entry.path(), target_path)?;
            }
        }
    }
    Ok(())
}
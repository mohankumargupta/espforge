use std::fs;
use std::path::Path;

use crate::{codegen::espgenerate::esp_generate, parse::EspforgeConfiguration};
use anyhow::{Context, Result};
use toml_edit::{DocumentMut, InlineTable, Item, Value};

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

    let platform_dir = project_dir.join("espforge_platform");
    crate::PLATFORM_SRC.extract(&platform_dir).context("Failed to extract espforge_platform")?;

    let app_rust_src = base_dir.join("app/rust/app.rs");
    if app_rust_src.exists() {
        fs::copy(&app_rust_src, src_dir.join("app.rs"))
            .context("Failed to copy app.rs to src/app.rs")?;
        println!("Copied app logic to src/app.rs");
    } else {
        println!("Warning: No app/rust/app.rs found.");
    }

    let cargo_toml_path = project_dir.join("Cargo.toml");
    let cargo_toml_content = fs::read_to_string(&cargo_toml_path)?;
    let mut manifest = cargo_toml_content.parse::<DocumentMut>()?;

    let mut platform_dep = InlineTable::default();
    platform_dep.insert("path", "espforge_platform".into());
    manifest["dependencies"]["espforge_platform"] = Item::Value(Value::InlineTable(platform_dep));
    fs::write(&cargo_toml_path, manifest.to_string())?;

    let main_rs_content = espforge_templates::render_main(None, "", "")
        .map_err(|e| anyhow::anyhow!("Failed to render template: {}", e))?;

    let main_rs_path = src_dir.join("bin/main.rs");
    if main_rs_path.exists() {
        fs::write(&main_rs_path, main_rs_content)
            .context("Failed to write generated main.rs")?;
        println!("✨ wired up main.rs");
    }

    Ok(())
}


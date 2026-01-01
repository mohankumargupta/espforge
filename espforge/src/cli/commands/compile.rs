use std::fs;
use std::path::Path;

use crate::{codegen::espgenerate::esp_generate, parse::EspforgeConfiguration};
use anyhow::{Context, Result};
use espforge_rust_app::parse_app_rs;

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
        let app_rust_dir = base_dir.join("app/rust");
    if app_rust_dir.exists() && app_rust_dir.is_dir() {
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        let project_dir = current_dir.join(config.get_name());
        let src_app_dir = project_dir.join("src/app");
        
        fs::create_dir_all(&src_app_dir).context("Failed to create src/app directory")?;
        copy_dir_recursive(&app_rust_dir, &src_app_dir).context("Failed to copy app/rust content")?;
        println!("Copied app/rust content to {}", src_app_dir.display());
    }

    let app_rs_path = base_dir.join("app/rust/app.rs");

    if app_rs_path.exists() {
        println!("Found app logic at: {}", app_rs_path.display());

        let app_code = parse_app_rs(&app_rs_path).context("Failed to parse app.rs")?;

        let main_rs_content =
            espforge_templates::render_main(None, &app_code.setup, &app_code.forever)
                .map_err(|e| anyhow::anyhow!("Failed to render template: {}", e))?;

        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        let project_dir = current_dir.join(config.get_name());
        let main_rs_path = project_dir.join("src/bin/main.rs");

        if main_rs_path.exists() {
            fs::write(&main_rs_path, main_rs_content)
                .context("Failed to write generated main.rs")?;
            println!("✨ Injected app logic into {}", main_rs_path.display());
        } else {
            println!(
                "Warning: Could not find generated main.rs at {}",
                main_rs_path.display()
            );
        }
    } else {
        println!(
            "No app/rust/app.rs found (checked {}), skipping logic injection.",
            app_rs_path.display()
        );
    }
    
    // let app_rs_path = base_dir.join("app/rust/app.rs");

    // if app_rs_path.exists() {
    //     println!("Found app logic at: {}", app_rs_path.display());

    //     let app_code = parse_app_rs(&app_rs_path).context("Failed to parse app.rs")?;

    //     let main_rs_content =
    //         espforge_templates::render_main(None, &app_code.setup, &app_code.forever)
    //             .map_err(|e| anyhow::anyhow!("Failed to render template: {}", e))?;

    //     let current_dir = std::env::current_dir().context("Failed to get current directory")?;
    //     let project_dir = current_dir.join(config.get_name());
    //     let main_rs_path = project_dir.join("src/bin/main.rs");

    //     if main_rs_path.exists() {
    //         fs::write(&main_rs_path, main_rs_content)
    //             .context("Failed to write generated main.rs")?;
    //         println!("✨ Injected app logic into {}", main_rs_path.display());
    //     } else {
    //         println!(
    //             "Warning: Could not find generated main.rs at {}",
    //             main_rs_path.display()
    //         );
    //     }
    // } else {
    //     println!(
    //         "No app/rust/app.rs found (checked {}), skipping logic injection.",
    //         app_rs_path.display()
    //     );
    // }

    Ok(())
}

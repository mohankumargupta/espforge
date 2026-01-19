use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn copy_wokwi_files(base_dir: &Path, project_dir: &Path) -> Result<()> {
    // List of files to copy if they exist in the source directory
    let files_to_copy = ["wokwi.toml", "diagram.json", "chip.json", "chip.wasm"];

    for filename in files_to_copy {
        let source_path = base_dir.join(filename);
        if source_path.exists() {
            let dest_path = project_dir.join(filename);
            fs::copy(&source_path, &dest_path)
                .with_context(|| format!("Failed to copy {} to project", filename))?;
            println!("   Included custom file: {}", filename);
        }
    }

    // If chip files are present (just copied), ensure wokwi.toml is updated to load them
    let chip_wasm = project_dir.join("chip.wasm");
    let chip_json = project_dir.join("chip.json");
    if chip_wasm.exists() && chip_json.exists() {
        update_wokwi_config_for_chip(project_dir)?;
    }

    Ok(())
}

fn update_wokwi_config_for_chip(project_dir: &Path) -> Result<()> {
    let wokwi_path = project_dir.join("wokwi.toml");
    if !wokwi_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&wokwi_path).context("Failed to read wokwi.toml")?;
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .context("Failed to parse wokwi.toml")?;

    let chips = doc.entry("chip").or_insert(toml_edit::Item::ArrayOfTables(
        toml_edit::ArrayOfTables::new(),
    ));

    if let Some(arr) = chips.as_array_of_tables_mut() {
        let exists = arr
            .iter()
            .any(|t| t.get("name").and_then(|s| s.as_str()) == Some("chip"));
        if !exists {
            let mut table = toml_edit::Table::new();
            table.insert("name", toml_edit::value("chip"));
            table.insert("binary", toml_edit::value("chip.wasm"));
            arr.push(table);
            fs::write(wokwi_path, doc.to_string())?;
            println!("   Updated wokwi.toml to include chip.wasm");
        }
    }
    Ok(())
}

pub fn provision_platform_assets(_project_dir: &Path, _src_dir: &Path) -> Result<()> {
    // Platform assets are now handled via Git dependencies in Cargo.toml
    // No local extraction required.
    Ok(())
}

pub fn inject_app_code(base_dir: &Path, src_dir: &Path) -> Result<()> {
    let rust_source = base_dir.join("app/rust/app.rs");
    let target = src_dir.join("app.rs");

    if rust_source.exists() {
        fs::copy(&rust_source, &target).context("Failed to copy app.rs")?;
        println!("   Included app logic from app/rust/app.rs");
    } else {
        println!("⚠️  Warning: No app code found. Generating stub.");
    }
    Ok(())
}

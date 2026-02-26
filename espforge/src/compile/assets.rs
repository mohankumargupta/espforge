use anyhow::{Context, Ok, Result};
use espforge_configuration::EspforgeConfiguration;
use espforge_esp32metadata::BoardDatabase;
use std::fs;
use std::path::Path;

pub fn generate_wokwi_config(
    project_dir: &Path,
    model: &EspforgeConfiguration,
) -> Result<()> {
    let diagram_json = project_dir.join("diagram.json");
    if !diagram_json.exists() {
        return Ok(());
    }

    let db = BoardDatabase::load();
    let chip = model.get_chip();

    let board_id = db
        .wokwi_board(chip)
        .ok_or_else(|| anyhow::anyhow!("Unsupported chip variant for Wokwi: {}", chip))?;

    let content =
        fs::read_to_string(&diagram_json).context("Failed to read diagram.json template")?;

    // Only process placeholders the first time. Leave it alone on subsequent runs.
    if !content.contains("PLACEHOLDER_WOKWI_BOARD") {
        let dest_path = project_dir.join("diagram.json");
        if diagram_json != dest_path {
            fs::copy(&diagram_json, &dest_path)?;
        }
        return Ok(());
    }

    // Replace board type placeholder (both formats for compatibility)
    let mut processed = content.replace("PLACEHOLDER_WOKWI_BOARD", &board_id);

    // Replace GND pin placeholders
    if let Some(gnd_tl) = db.gnd_top_left(chip) {
        processed = processed.replace("PLACEHOLDER_GND_TOP_LEFT", &gnd_tl);
    }
    if let Some(gnd_tr) = db.gnd_top_right(chip) {
        processed = processed.replace("PLACEHOLDER_GND_TOP_RIGHT", &gnd_tr);
    }
    if let Some(gnd_bl) = db.gnd_bottom_left(chip) {
        processed = processed.replace("PLACEHOLDER_GND_BOTTOM_LEFT", &gnd_bl);
    }
    if let Some(gnd_br) = db.gnd_bottom_right(chip) {
        processed = processed.replace("PLACEHOLDER_GND_BOTTOM_RIGHT", &gnd_br);
    }

    serde_json::from_str::<serde_json::Value>(&processed)
        .context("Processed diagram.json is not valid JSON")?;

    fs::write(project_dir.join("diagram.json"), processed)
        .context("Failed to write processed diagram.json")?;

    println!("   ✓ Processed diagram.json for {}", board_id);
    Ok(())
}

pub fn copy_wokwi_files(project_dir: &Path) -> Result<()> {
    // for filename in &["wokwi.toml"] {  // diagram.json handled by generate_wokwi_config
    //     let source_path = project_dir.join(filename);
    //     if source_path.exists() {
    //         let dest_path = project_dir.join(filename);
    //         fs::copy(&source_path, &dest_path)
    //             .with_context(|| format!("Failed to copy {}", filename))?;
    //         println!("   Included custom file: {} (overriding generated)", filename);
    //     }
    // }

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

pub fn inject_app_code(src_dir: &Path) -> Result<Vec<String>> {
    let mut module_names = vec![];
    if src_dir.exists() {
        for entry in fs::read_dir(src_dir).context("Failed to read src directory")? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let file_name_str = path.file_name().unwrap().to_string_lossy().to_string();
                if let Some(module_name) = file_name_str.strip_suffix(".rs") {
                    if module_name != "lib"
                        && module_name != "generated"
                        && module_name != "main"
                        && !module_name.starts_with("bin")
                    {
                        module_names.push(module_name.to_string());
                    }
                }
            }
        }
    }
    module_names.sort();
    module_names.dedup();
    Ok(module_names)
}

use crate::manifest::ComponentManifest;
use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use std::collections::HashMap;
use std::fs;
use std::path::Path; // Added Path import
use std::process::Command;
use toml;

static COMPONENTS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/components");
static GLOBALS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/globals");
static PLATFORM_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/platform");
static DEVICES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/devices");

pub fn load_manifests() -> Result<HashMap<String, ComponentManifest>> {
    let mut manifests = HashMap::new();

    // Helper to load manifests from a directory recursively
    let mut load_from_dir = |dir: &Dir<'_>| -> Result<()> {
        for entry in dir.find("**/*.ron")? {
            if let Some(file) = entry.as_file() {
                let path_str = file.path().to_str().unwrap_or("unknown");

                let content = file
                    .contents_utf8()
                    .with_context(|| format!("File {} is not valid UTF-8", path_str))?;

                let manifest: ComponentManifest = ron::from_str(content)
                    .with_context(|| format!("Failed to parse manifest file: {}", path_str))?;
                manifests.insert(manifest.name.clone(), manifest);
            }
        }
        Ok(())
    };

    load_from_dir(&COMPONENTS_DIR)?;
    load_from_dir(&GLOBALS_DIR)?;
    load_from_dir(&DEVICES_DIR)?;
    Ok(manifests)
}

pub fn generate(project_name: &str, chip: &str, enable_async : bool) -> Result<()> {
    if fs::metadata(project_name).is_ok() {
        anyhow::bail!("Directory {} already exists", project_name);
    }

    let mut cmd = Command::new("esp-generate");
    cmd.arg("--headless")
        .arg("--chip")
        .arg(chip)
        .arg("-o")
        .arg("log")
        .arg("-o")
        .arg("unstable-hal")
        .arg("-o")
        .arg("esp-backtrace")
        .arg("-o")
        .arg("wokwi")
        .arg("-o")
        .arg("vscode");
    if enable_async {
        cmd.arg("-o").arg("embassy");
    }

    let status = cmd.arg(project_name).output()?;

    if !status.status.success() {
        anyhow::bail!(
            "esp-generate failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    // Create src/components directory
    let components_path = format!("{}/src/components", project_name);
    fs::create_dir_all(&components_path)?;

    // Copy component sources
    for file in COMPONENTS_DIR.files() {
        let path = format!("{}/{}", components_path, file.path().to_str().unwrap());
        if path.ends_with(".rs") {
            fs::write(&path, file.contents())?;
        }
    }

    // Create src/platform directory and copy sources
    let platform_path = format!("{}/src/platform", project_name);
    fs::create_dir_all(&platform_path)?;

    for file in PLATFORM_DIR.files() {
        let path = format!("{}/{}", platform_path, file.path().to_str().unwrap());
        fs::write(&path, file.contents())?;
    }

    // Create src/globals directory and copy sources
    let globals_path = format!("{}/src/globals", project_name);
    fs::create_dir_all(&globals_path)?;

    // Devices
    let devices_path = format!("{}/src/devices", project_name);
    fs::create_dir_all(&devices_path)?;

    // Flatten nested devices (e.g. devices/ssd1306/device.rs -> src/devices/ssd1306.rs)
    let mut device_modules = Vec::new();
    for subdir in DEVICES_DIR.dirs() {
        if let Some(device_name) = subdir.path().file_name().and_then(|n| n.to_str()) {
            for file in subdir.files() {
                if file.path().file_name().and_then(|n| n.to_str()) == Some("device.rs") {
                    let dest = format!("{}/{}.rs", devices_path, device_name);
                    fs::write(&dest, file.contents())?;
                    device_modules.push(device_name.to_string());
                }
            }
        }
    }

    // Create src/devices/mod.rs
    let mut devices_mod_content = String::new();
    for module in device_modules {
        devices_mod_content.push_str(&format!("pub mod {};\n", module));
        // Add pub use to re-export the device struct
        devices_mod_content.push_str(&format!("pub use {}::*;\n", module));
    }
    fs::write(format!("{}/mod.rs", devices_path), devices_mod_content)?;

    let mut global_modules = Vec::new();

    for file in GLOBALS_DIR.files() {
        let file_path_str = file.path().to_str().unwrap();
        
        // Skip async_signal if not enabled
        if !enable_async && file_path_str.starts_with("async_signal") {
            continue;
        }

        if file_path_str == "mod.rs" {
            continue;
        }

        let path = format!("{}/{}", globals_path, file_path_str);
        if path.ends_with(".rs") {
            fs::write(&path, file.contents())?;
            if let Some(stem) = Path::new(file_path_str).file_stem().and_then(|s| s.to_str()) {
                global_modules.push(stem.to_string());
            }
        }
    }

    // Generate mod.rs dynamically based on copied files
    // CHANGE: Added #![allow(unused_imports)] to suppress warnings for unused global modules
    let mut globals_mod_content = String::from("#![allow(unused_imports)]\n");
    for mod_name in global_modules {
        globals_mod_content.push_str(&format!("pub mod {};\npub use {}::*;\n", mod_name, mod_name));
    }
    fs::write(format!("{}/mod.rs", globals_path), globals_mod_content)?;

    // Update Cargo.toml: Merge device dependencies and add [workspace]
    update_cargo_manifest(project_name)?;

    Ok(())
}

fn update_cargo_manifest(project_name: &str) -> Result<()> {
    let cargo_path = format!("{}/Cargo.toml", project_name);
    let cargo_content = fs::read_to_string(&cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path))?;

    // Parse the generated Cargo.toml
    let mut root_manifest: toml::Table =
        toml::from_str(&cargo_content).with_context(|| "Failed to parse generated Cargo.toml")?;

    // 1. Add [workspace] to the bottom (via Table insertion)
    // This effectively isolates the project from any parent Cargo.toml files
    if !root_manifest.contains_key("workspace") {
        root_manifest.insert(
            "workspace".to_string(),
            toml::Value::Table(toml::Table::new()),
        );
    }

    // 2. Merge dependencies from all devices found in DEVICES_DIR
    // We assume the destination 'dependencies' table exists (esp-generate creates it)
    let root_deps = root_manifest
        .entry("dependencies".to_string())
        .or_insert(toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("'dependencies' in Cargo.toml is not a table"))?;

    for subdir in DEVICES_DIR.dirs() {
        let cargo_file_opt = subdir
            .files()
            .find(|f| f.path().file_name().and_then(|n| n.to_str()) == Some("Cargo.toml.tera"));

        if let Some(cargo_file) = cargo_file_opt {
            let device_toml_str = cargo_file
                .contents_utf8()
                .ok_or_else(|| anyhow::anyhow!("Device Cargo.toml.tera is not valid UTF-8"))?;

            // Parse the device manifest
            let device_manifest: toml::Value =
                toml::from_str(device_toml_str).with_context(|| {
                    format!("Failed to parse Cargo.toml.tera for device {:?}", subdir.path())
                })?;

            // If it has dependencies, merge them into the root dependencies
            if let Some(deps) = device_manifest
                .get("dependencies")
                .and_then(|d| d.as_table())
            {
                for (k, v) in deps {
                    root_deps.insert(k.clone(), v.clone());
                }
            }
        }
    }

    // Write back to file
    let new_content = toml::to_string_pretty(&root_manifest)?;
    fs::write(&cargo_path, new_content)?;

    Ok(())
}


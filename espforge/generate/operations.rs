// generate/file_operations.rs - File and directory operations

use super::manifest;
use anyhow::{Context, Result};
use include_dir::Dir;
use log::{debug, info};
use std::fs;
use std::path::Path;

pub fn copy_components(src_path: &Path) -> Result<()> {
    let components_path = src_path.join("components");
    copy_dir_files(
        manifest::components_dir(), 
        &components_path, 
        |path| path.ends_with(".rs")
    ).context("Failed to copy components")?;
    info!("Copied component files");
    Ok(())
}

pub fn copy_devices(src_path: &Path) -> Result<()> {
    let devices_path = src_path.join("devices");
    fs::create_dir_all(&devices_path)
        .with_context(|| format!("Failed to create directory: {}", devices_path.display()))?;

    let mut modules = Vec::new();

    for subdir in manifest::devices_dir().dirs() {
        let device_name = subdir.path().file_name().unwrap().to_str().unwrap();
        
        // Find device.rs in the device subdirectory (e.g. devices/ili9341/device.rs)
        if let Some(file) = subdir.files().find(|f| f.path().file_name().and_then(|n| n.to_str()) == Some("device.rs")) {
            // Copy it to src/devices/<device_name>.rs
            let dest_file = devices_path.join(format!("{}.rs", device_name));
            fs::write(&dest_file, file.contents())
                .with_context(|| format!("Failed to write device file: {}", dest_file.display()))?;
            modules.push(device_name.to_string());
            debug!("Copied device: {}", device_name);
        }
    }
    
    // Sort modules for deterministic output
    modules.sort();

    // Generate src/devices/mod.rs
    generate_mod_file(&devices_path, &modules, true)?;
    info!("Copied device files");
    Ok(())
}

pub fn copy_platform_files(src_path: &Path) -> Result<()> {
    let platform_path = src_path.join("platform");
    copy_dir_files(manifest::platform_dir(), &platform_path, |_| true)
        .context("Failed to copy platform files")?;
    info!("Copied platform files");
    Ok(())
}

pub fn copy_dir_files<F>(dir: &Dir<'_>, dest_path: &Path, filter: F) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    fs::create_dir_all(dest_path)
        .with_context(|| format!("Failed to create directory: {}", dest_path.display()))?;

    for file in dir.files() {
        let file_path_str = file.path().to_str().context("Invalid UTF-8 in file path")?;
        
        if filter(file_path_str) {
            let dest_file = dest_path.join(file.path());
            fs::write(&dest_file, file.contents())
                .with_context(|| format!("Failed to write file: {}", dest_file.display()))?;
            debug!("Copied: {}", file_path_str);
        }
    }
    
    Ok(())
}

pub fn copy_globals_files(src_path: &Path) -> Result<()> {
    let globals_path = src_path.join("globals");
    copy_dir_files(
        manifest::globals_dir(), 
        &globals_path, 
        |path| path.ends_with(".rs")
    ).context("Failed to copy global files")?;
    info!("Copied global files");
    Ok(())
}


pub fn generate_mod_file(dir_path: &Path, modules: &[String], use_pub_use: bool) -> Result<()> {
    let mut content = String::from("#![allow(unused_imports)]\n");
    
    for module in modules {
        content.push_str(&format!("pub mod {};\n", module));
        if use_pub_use {
            content.push_str(&format!("pub use {}::*;\n", module));
        } else {
            content.push_str(&format!("pub use {}::*;\n", module));
        }
    }
    
    let mod_file = dir_path.join("mod.rs");
    fs::write(&mod_file, content)
        .with_context(|| format!("Failed to write mod file: {}", mod_file.display()))?;
    
    debug!("Generated mod.rs with {} modules", modules.len());
    Ok(())
}

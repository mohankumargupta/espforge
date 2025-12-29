use anyhow::{Result, anyhow};
use std::{env, fs, path::Path};
use toml_edit::{DocumentMut};

pub fn main() -> Result<()> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    if let Some(path) = Path::new(&manifest_dir).parent() {
        let get_dep_version = |doc: DocumentMut| -> Result<String> {
            doc.get("package")
                .and_then(|p| p.get("version"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow!("Field [package] version not found in Cargo.toml"))
        };

        let espforge_platform = path.join("espforge_platform");
        let espforge_platform_cargo = espforge_platform.join("Cargo.toml");
        let espforge_platform_cargo_content = fs::read_to_string(espforge_platform_cargo)?;
        let espforge_platform_doc = espforge_platform_cargo_content.parse::<DocumentMut>()?;

        let espforge_devices = path.join("espforge_devices");
        let espforge_devices_cargo = espforge_devices.join("Cargo.toml");
        let espforge_devices_cargo_content = fs::read_to_string(espforge_devices_cargo)?;
        let espforge_devices_doc = espforge_devices_cargo_content.parse::<DocumentMut>()?;

        let platform_version = get_dep_version(espforge_platform_doc)?;
        let devices_version = get_dep_version(espforge_devices_doc)?;
        // println!("cargo:warning={platform_ver}!");
        // println!("cargo:warning={devices_ver}!");
        println!("cargo:rustc-env=ESPFORGE_PLATFORM_VERSION={}", platform_version);
        println!("cargo:rustc-env=ESPFORGE_DEVICES_VERSION={}", devices_version);
    }
    Ok(())
}

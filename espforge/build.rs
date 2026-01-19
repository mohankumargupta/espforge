use anyhow::{Result, anyhow};
use std::{env, fs, path::Path};
use toml_edit::{DocumentMut, Item};

pub fn main() -> Result<()> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    if let Some(path) = Path::new(&manifest_dir).parent() {
        let espforge_platform = path.join("espforge_platform");
        let espforge_platform_cargo = espforge_platform.join("Cargo.toml");
        //println!("cargo:warning={:?}!", espforge_platform_cargo);
        let espforge_platform_cargo_content = fs::read_to_string(espforge_platform_cargo)?;
        let espforge_platform_doc = espforge_platform_cargo_content.parse::<DocumentMut>()?;
        let get_dep_version = |dep_name: &str| -> Result<String> {
            let deps = espforge_platform_doc
                .get("dependencies")
                .and_then(|d| d.as_table())
                .ok_or_else(|| anyhow!("No [dependencies] section found"))?;

            let item = deps
                .get(dep_name)
                .ok_or_else(|| anyhow!("Dependency '{}' not found", dep_name))?;

            // Handle { version = "..." } or simple "..."
            let version = match item {
                Item::Value(v) => v.as_str().map(|s| s.to_string()),
                Item::Table(t) => t
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            };

            version.ok_or_else(|| anyhow!("Could not determine version for '{}'", dep_name))
        };

        let espforge_devices = path.join("espforge_devices");
        let espforge_devices_cargo = espforge_devices.join("Cargo.toml");
        //println!("cargo:warning={:?}!", espforge_devices_cargo);
    }
    Ok(())
}

use anyhow::{Context, Result, anyhow};
use std::{env, fs, path::Path};
use toml_edit::{DocumentMut, value};

pub fn main() -> Result<()> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let base_path = Path::new(&manifest_dir);

    // This is the file that will travel with the published crate
    let snapshot_path = base_path.join("embedded_versions.toml");

    // 1. Try to sync from Workspace (Development Mode)
    // We attempt to read the sibling crates. If they exist, we update our snapshot.
    // If they don't exist (e.g. inside cargo install), we skip this and use the snapshot.
    if let Some(workspace_root) = base_path.parent() {
        let platform_cargo = workspace_root.join("espforge_platform").join("Cargo.toml");
        let components_cargo = workspace_root
            .join("espforge_components")
            .join("Cargo.toml");
        let devices_cargo = workspace_root.join("espforge_devices").join("Cargo.toml");

        if platform_cargo.exists() && devices_cargo.exists() {
            let platform_ver = get_version_from_cargo(&platform_cargo)?;
            let components_ver = get_version_from_cargo(&components_cargo)?;
            let devices_ver = get_version_from_cargo(&devices_cargo)?;

            // Create TOML content
            let mut snapshot = DocumentMut::new();
            snapshot["platform_version"] = value(platform_ver.clone());
            snapshot["components_version"] = value(components_ver.clone());
            snapshot["devices_version"] = value(devices_ver.clone());

            let new_content = snapshot.to_string();

            // Only write if changed to avoid unnecessary rebuild loops
            let needs_write = if snapshot_path.exists() {
                fs::read_to_string(&snapshot_path)? != new_content
            } else {
                true
            };

            if needs_write {
                fs::write(&snapshot_path, new_content)?;
            }

            // Watch sibling files for changes
            println!("cargo:rerun-if-changed={}", platform_cargo.display());
            println!("cargo:rerun-if-changed={}", components_cargo.display());
            println!("cargo:rerun-if-changed={}", devices_cargo.display());
        }
    }

    // 2. Read from Snapshot (Install/Publish Mode)
    // At this point, snapshot_path MUST exist (either just written, or unpacked from crate)
    if !snapshot_path.exists() {
        // Fallback for clean checkouts where build hasn't run yet
        // and siblings are missing (unlikely in valid dev setups)
        println!("cargo:warning=embedded_versions.toml not found. Run a local build first.");
        return Ok(());
    }

    let snapshot_content =
        fs::read_to_string(&snapshot_path).context("Failed to read embedded_versions.toml")?;
    let doc = snapshot_content.parse::<DocumentMut>()?;

    let p_ver = doc["platform_version"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing platform_version"))?;
    let c_ver = doc["components_version"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing component_version"))?;
    let d_ver = doc["devices_version"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing devices_version"))?;

    println!("cargo:rustc-env=ESPFORGE_PLATFORM_VERSION={}", p_ver);
    println!("cargo:rustc-env=ESPFORGE_COMPONENTS_VERSION={}", c_ver);
    println!("cargo:rustc-env=ESPFORGE_DEVICES_VERSION={}", d_ver);
    println!("cargo:rerun-if-changed=embedded_versions.toml");
    println!("cargo:rerun-if-changed=dependencies.toml");

    Ok(())
}

fn get_version_from_cargo(path: &Path) -> Result<String> {
    let content = fs::read_to_string(path)?;
    let doc = content.parse::<DocumentMut>()?;
    doc.get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Version not found in {}", path.display()))
}

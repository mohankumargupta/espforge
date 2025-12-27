use crate::manifest::ComponentManifest;
use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use log::{debug, info};
use std::collections::HashMap;

static COMPONENTS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/components");
static GLOBALS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/globals");
static DEVICES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/devices");

pub fn load_manifests() -> Result<HashMap<String, ComponentManifest>> {
    let mut manifests = HashMap::new();

    load_from_dir(&COMPONENTS_DIR, &mut manifests)?;
    load_from_dir(&GLOBALS_DIR, &mut manifests)?;
    load_from_dir(&DEVICES_DIR, &mut manifests)?;
    
    info!("Loaded {} component manifests", manifests.len());
    Ok(manifests)
}

fn load_from_dir(
    dir: &Dir<'_>, 
    manifests: &mut HashMap<String, ComponentManifest>
) -> Result<()> {
    for entry in dir.find("**/*.ron")? {
        if let Some(file) = entry.as_file() {
            let path_str = file.path().to_str().context("Invalid UTF-8 in file path")?;

            let content = file
                .contents_utf8()
                .with_context(|| format!("File {} is not valid UTF-8", path_str))?;

            let manifest: ComponentManifest = ron::from_str(content)
                .with_context(|| format!("Failed to parse manifest file: {}", path_str))?;
            
            debug!("Loaded manifest: {}", manifest.name);
            manifests.insert(manifest.name.clone(), manifest);
        }
    }
    Ok(())
}

pub(crate) fn devices_dir() -> &'static Dir<'static> {
    &DEVICES_DIR
}

pub(crate) fn components_dir() -> &'static Dir<'static> {
    &COMPONENTS_DIR
}

pub(crate) fn platform_dir() -> &'static Dir<'static> {
    &PLATFORM_DIR
}

pub(crate) fn globals_dir() -> &'static Dir<'static> {
    &GLOBALS_DIR
}

static PLATFORM_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/platform");

use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use std::fs;
use std::path::Path;
use toml_edit::DocumentMut;

mod core;
mod external;
mod features;
mod versioning;

use core::CoreDependencies;
use external::ExternalDependencies;
use features::FeatureManager;
use versioning::VersionResolver;

pub fn add_dependencies(
    project_dir: &Path,
    model: &EspforgeConfiguration,
    config_dir: &Path,
) -> Result<()> {
    let cargo_path = project_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;

    let mut doc: DocumentMut = manifest.parse().context("Failed to parse Cargo.toml")?;

    let versions = VersionResolver::new()?;

    CoreDependencies::add(&mut doc, &versions)?;

    FeatureManager::add_platform_features(&mut doc, model)?;

    FeatureManager::add_component_features(&mut doc, model)?;

    FeatureManager::add_device_features(&mut doc, model)?;

    FeatureManager::handle_embassy_features(&mut doc, model)?;

    ExternalDependencies::merge_embedded(&mut doc, model)?;

    ExternalDependencies::merge_external(&mut doc, config_dir)?;

    FeatureManager::handle_psram(&mut doc, model)?;

    fs::write(cargo_path, doc.to_string()).context("Failed to write Cargo.toml")?;

    Ok(())
}

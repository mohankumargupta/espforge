use std::fs;
use std::path::Path;

use crate::{codegen::espgenerate::esp_generate, parse::EspforgeConfiguration};
use anyhow::{Context, Result};

pub fn execute(file: &Path) -> Result<()> {
    let content = fs::read_to_string(file).context(format!(
        "Failed to read configuration file: {}",
        file.display()
    ))?;

    let config: EspforgeConfiguration =
        serde_yaml_ng::from_str(&content).context("Failed to parse configuration")?;

    println!("Configuration valid.");

    esp_generate(config.get_name(), &config.get_chip(), false)

    //Ok(())
}

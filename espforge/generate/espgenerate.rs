use anyhow::{Context, Result};
use log::debug;
use std::process::Command;

pub fn run(project_name: &str, chip: &str, enable_async: bool) -> Result<()> {
    debug!("Running esp-generate for chip: {}", chip);
    
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
        debug!("Enabling embassy async support");
        cmd.arg("-o").arg("embassy");
    }

    let output = cmd.arg(project_name).output()
        .context("Failed to execute esp-generate command")?;

    if !output.status.success() {
        anyhow::bail!(
            "esp-generate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    
    Ok(())
}
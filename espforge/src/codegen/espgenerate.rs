use anyhow::{Context, Result};
use std::process::Command;

pub fn esp_generate(project_name: &str, chip: &str, enable_async: bool) -> Result<()> {
    println!("Running esp-generate for chip: {}", chip);
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

    let output = cmd
        .arg(project_name)
        .output()
        .context("Failed to execute esp-generate command")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "esp-generate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

pub fn esp_generate(
    project_name: &str,
    chip: &str,
    enable_async: bool,
    enable_wifi: bool,
    project_dir: &Path,
) -> Result<()> {
    println!("Running esp-generate for chip: {}", chip);
    let temp_base = std::env::temp_dir().join(format!(
        "espforge_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&temp_base).context("Failed to create temp directory")?;

    let mut cmd = Command::new("esp-generate");
    cmd.current_dir(&temp_base);
    #[rustfmt::skip]
    cmd.args([
        "--headless",
        "--chip", chip,
        "-o", "log",
        "-o", "unstable-hal",
        "-o",  "alloc",
        "-o", "esp-backtrace",
        "-o", "wokwi",
        "-o", "vscode",
    ]);

    if enable_async {
        cmd.arg("-o").arg("embassy");
    }

    if enable_wifi {
        cmd.arg("-o").arg("wifi");
    }

    cmd.arg(project_name);

    let output = cmd
        .output()
        .context("Failed to execute esp-generate command")?;

    if !output.status.success() {
        let _ = fs::remove_dir_all(&temp_base);
        return Err(anyhow::anyhow!(
            "esp-generate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let generated_dir = temp_base.join(project_name);
    merge_directories(&generated_dir, project_dir)?;
    let _ = fs::remove_dir_all(&temp_base);

    Ok(())
}

fn merge_directories(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            merge_directories(&entry.path(), &dst_path)?;
        } else if !dst_path.exists() {
            // Only copy if the file DOES NOT exist in the current directory
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

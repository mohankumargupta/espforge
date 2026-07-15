//! Scaffold a standard Espressif no_std project via the external `esp-generate`
//! tool, then merge it into the output dir without clobbering espforge's own
//! files (ADR-001: espforge owns only the files it emits; esp-generate's
//! scaffold and user overrides are left alone on re-run). This mirrors v1's
//! `espforge_codegen::scaffold::esp_generate`.

use anyhow::{Context, Result};
use espforge_model::ir::DeviceTree;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

pub fn scaffold(ir: &DeviceTree, out_dir: &Path) -> Result<()> {
    let chip = ir
        .meta
        .target
        .clone()
        .unwrap_or_else(|| "esp32".to_string());
    let name = ir.meta.name.clone().unwrap_or_else(|| "espforge-project".into());
    let project_name = name.replace('-', "_");

    // Build the esp-generate argument list explicitly so we can both execute
    // it and print the exact command (with every `-o` option). Design note:
    // the user asked to see the precise invocation espforge runs, not a hint.
    let mut args: Vec<String> = vec![
        "--headless".into(),
        "--chip".into(),
        chip.clone(),
        "-o".into(),
        "log".into(),
        "-o".into(),
        "unstable-hal".into(),
        "-o".into(),
        "alloc".into(),
        "-o".into(),
        "esp-backtrace".into(),
        "-o".into(),
        "wokwi".into(),
        "-o".into(),
        "vscode".into(),
    ];
    if ir.flags.is_embassy {
        args.push("-o".into());
        args.push("embassy".into());
    }
    if ir.flags.has_wifi {
        args.push("-o".into());
        args.push("wifi".into());
    }
    args.push(project_name.clone());

    // Render the command the way the user would type it (binary + every arg
    // quoted so paths/options survive shell parsing), and echo it up front so
    // the build is reproducible.
    let rendered = render_command("esp-generate", &args);
    println!("scaffolding via: {rendered}");

    let temp_base = std::env::temp_dir().join(format!(
        "espforge_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&temp_base).context("failed to create temp dir")?;

    let mut cmd = Command::new("esp-generate");
    cmd.current_dir(&temp_base).args(&args);

    let output = cmd
        .output()
        .context("failed to execute esp-generate (is it installed? run: cargo binstall esp-generate)")?;
    if !output.status.success() {
        let _ = fs::remove_dir_all(&temp_base);
        return Err(anyhow::anyhow!(
            "esp-generate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let generated_dir = temp_base.join(&project_name);
    merge_directories(&generated_dir, out_dir)?;
    let _ = fs::remove_dir_all(&temp_base);
    Ok(())
}

/// Render a command line for display: `binary` followed by each argument,
/// space-separated. Used so the user sees the exact `esp-generate` invocation
/// espforge runs. Arguments here are simple tokens (chip names, options, a
/// project name), so no shell quoting is needed.
fn render_command(binary: &str, args: &[String]) -> String {
    let mut parts: Vec<String> = vec![binary.to_string()];
    parts.extend(args.iter().cloned());
    parts.join(" ")
}

/// Copy files from `src` into `dst`, but never overwrite a file that already
/// exists in `dst`. This preserves espforge's emitted files and any user
/// overrides (e.g. a hand-written `.cargo/config.toml`) on re-run.
fn merge_directories(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            merge_directories(&entry.path(), &dst_path)?;
        } else if !dst_path.exists() {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

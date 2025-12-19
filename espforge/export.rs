use crate::config::{EspforgeConfiguration, PlatformConfig};
use crate::template_utils::{copy_verbatim, find_template_path, get_templates, write_template};
use anyhow::{Context, Result, anyhow};
use std::path::Path;

pub struct ExportOptions {
    pub example_name: String,
    pub override_project_name: Option<String>,
    pub override_platform: Option<String>,
}

pub fn export_example(options: ExportOptions, target_dir: &Path) -> Result<String> {
    let root = get_templates();

    let template_path_str = find_template_path(&options.example_name)
        .ok_or_else(|| anyhow!("Example '{}' not found", options.example_name))?;

    let template_dir = root.get_dir(&template_path_str).ok_or_else(|| {
        anyhow!(
            "Template directory structure error for '{}'",
            options.example_name
        )
    })?;

    let example_file = template_dir.path().join("example.yaml");
    // Use root to get file by full path to ensure correct lookup
    let yaml_file = root
        .get_file(&example_file)
        .ok_or_else(|| anyhow!("Template is missing example.yaml"))?;

    let raw_yaml = yaml_file
        .contents_utf8()
        .context("Invalid UTF-8 in example.yaml")?;

    let mut config: EspforgeConfiguration = serde_yaml_ng::from_str(raw_yaml)
        .context("Failed to parse example.yaml into configuration")?;

    if let Some(name) = options.override_project_name {
        config.espforge.name = name;
    }

    if let Some(platform_str) = options.override_platform {
        let platform_enum: PlatformConfig = serde_yaml_ng::from_str(&platform_str)
            .map_err(|_| anyhow!("Invalid platform: {}", platform_str))?;
        config.espforge.platform = platform_enum;
    }

    let project_name = config.get_name().to_string();

    let yaml_filename = format!("{}.yaml", project_name);
    let yaml_dest = target_dir.join(&yaml_filename);

    let modified_yaml = serde_yaml_ng::to_string(&config)?;
    write_template(&yaml_dest, &modified_yaml)?;
    println!("Created config: {}", yaml_filename);

    let project_path_str = target_dir.to_string_lossy();
    let template_root_path = template_dir.path();

    for entry in template_dir.find("**/*")? {
        if let Some(file) = entry.as_file() {
            let file_path = file.path();
            let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name == "example.yaml" {
                continue;
            }

            if file_path
                .components()
                .any(|c| c.as_os_str() == ".zig-cache" || c.as_os_str() == "zig-out")
            {
                continue;
            }

            // We do not render templates here; that happens during 'compile'.
            copy_verbatim(file, template_root_path, &project_path_str)?;
            let relative_path = file_path
                .strip_prefix(template_root_path)
                .unwrap_or(file_path);
            println!("Created file: {}", relative_path.display());
        }
    }

    let target_cargo_tera = target_dir.join("Cargo.toml.tera");
    if !target_cargo_tera.exists() {
        if let Some(dynamic_cargo) = root.get_file("_dynamic/Cargo.toml.tera") {
             copy_verbatim(dynamic_cargo, Path::new("_dynamic"), &project_path_str)?;
             println!("Created file: Cargo.toml.tera (from _dynamic)");
        }
    }

    Ok(project_name)
}
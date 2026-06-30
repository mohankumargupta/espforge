use super::config::ConfigFile;
use super::fs::OutputDirectory;
use crate::cli::model::{ExampleConfig, ExportResult};
use anyhow::{Context, Result, anyhow};
use espforge_examples::EXAMPLES_DIR;
use std::fs;
use std::path::Path;

pub struct ExampleExporter;

impl Default for ExampleExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExampleExporter {
    pub fn new() -> Self {
        Self
    }

    pub fn export(&self, config: &ExampleConfig, output: &OutputDirectory) -> Result<ExportResult> {
        let template = ExampleTemplate::find(&config.template_name)?;
        template.extract_to(output.path())?;

        let app_rust_dir = output.path().join("app").join("rust");
        let src_dir = output.path().join("src");

        if app_rust_dir.exists() {
            if !src_dir.exists() {
                fs::create_dir_all(&src_dir).context("Failed to create src directory")?;
            }

            for entry in fs::read_dir(&app_rust_dir).context("Failed to read app/rust directory")? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file()
                    && let Some(file_name) = path.file_name()
                {
                    let dest = src_dir.join(file_name);
                    fs::rename(&path, &dest).context("Failed to move file to src")?;
                }
            }

            let app_dir = output.path().join("app");
            if app_dir.exists() {
                let _ = fs::remove_dir_all(&app_dir);
            }
        }

        let config_file = ConfigFile::locate(output.path())?;
        config_file.update(config)?;

        let final_name = config_file.rename_to(&config.project_name)?;

        Ok(ExportResult {
            project_name: config.project_name.clone(),
            output_file: format!("{}.yaml", final_name),
        })
    }
}

pub(crate) struct ExampleTemplate {
    dir: &'static include_dir::Dir<'static>,
}

impl ExampleTemplate {
    pub(crate) fn find(name: &str) -> Result<Self> {
        let dir = Self::search_in_catalog(name)
            .ok_or_else(|| anyhow!("Example template '{}' not found", name))?;

        Ok(Self { dir })
    }

    fn search_in_catalog(name: &str) -> Option<&'static include_dir::Dir<'static>> {
        EXAMPLES_DIR
            .dirs()
            .flat_map(|category| category.dirs())
            .find(|example| example.path().file_name().and_then(|n| n.to_str()) == Some(name))
    }

    fn extract_to(&self, target: &Path) -> Result<()> {
        extract_recursive(self.dir, target, self.dir.path())
            .context("Failed to extract example files to disk")
    }

    pub fn read_chip_from_yaml(&self) -> Option<String> {
        let yaml_file = self.dir.get_file("example.yaml")?;
        let content = std::str::from_utf8(yaml_file.contents()).ok()?;
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(content).ok()?;
        doc.get("espforge")
            .and_then(|e| {
                e.get("platform")
                    .or_else(|| e.get("chip"))
            })
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

fn extract_recursive(
    dir: &include_dir::Dir,
    base_path: &Path,
    root_prefix: &Path,
) -> std::io::Result<()> {
    // Logic to strip prefix and write files/directories recursively
    let dir_path = dir.path();
    let relative_dir_path = dir_path
        .strip_prefix(root_prefix)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let dest_dir = base_path.join(relative_dir_path);
    if !dest_dir.exists() {
        fs::create_dir_all(&dest_dir)?;
    }

    for file in dir.files() {
        let path = file.path();
        let relative_path = path
            .strip_prefix(root_prefix)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let dest_path = base_path.join(relative_path);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(dest_path, file.contents())?;
    }

    for subdir in dir.dirs() {
        extract_recursive(subdir, base_path, root_prefix)?;
    }

    Ok(())
}

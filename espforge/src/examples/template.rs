use anyhow::{Context, Result, anyhow};
use espforge_examples::EXAMPLES_DIR;
use std::fs;
use std::path::Path;
use crate::cli::model::{ExampleConfig, ExportResult};
use super::config::ConfigFile;
use super::fs::OutputDirectory;

pub struct ExampleExporter;

impl ExampleExporter {
    pub fn new() -> Self {
        Self
    }

    pub fn export(&self, config: &ExampleConfig, output: &OutputDirectory) -> Result<ExportResult> {
        let template = ExampleTemplate::find(&config.template_name)?;
        template.extract_to(output.path())?;

        let config_file = ConfigFile::locate(output.path())?;
        config_file.update(config)?;

        let final_name = config_file.rename_to(&config.project_name)?;

        Ok(ExportResult {
            project_name: config.project_name.clone(),
            output_file: format!("{}.yaml", final_name),
        })
    }
}

struct ExampleTemplate {
    dir: &'static include_dir::Dir<'static>,
}

impl ExampleTemplate {
    fn find(name: &str) -> Result<Self> {
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
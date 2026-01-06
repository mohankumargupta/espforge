use anyhow::{Context, Result, bail, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use crate::cli::model::ExampleConfig;

pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    const DEFAULT_NAME: &'static str = "example.yaml";

    pub fn locate(base_path: &Path) -> Result<Self> {
        let path = base_path.join(Self::DEFAULT_NAME);

        if !path.exists() {
            bail!("The example template is missing the required 'example.yaml' file");
        }

        Ok(Self { path })
    }

    pub fn update(&self, config: &ExampleConfig) -> Result<()> {
        let mut yaml_doc = self.read_yaml()?;

        Self::apply_project_name(&mut yaml_doc, &config.project_name);
        Self::apply_chip(&mut yaml_doc, &config.chip);

        self.write_yaml(&yaml_doc)
    }

    pub fn rename_to(&self, project_name: &str) -> Result<String> {
        let new_filename = format!("{}.yaml", project_name);
        let new_path = self
            .path
            .parent()
            .ok_or_else(|| anyhow!("Invalid config file path"))?
            .join(&new_filename);

        fs::rename(&self.path, &new_path).context("Failed to rename configuration file")?;

        Ok(project_name.to_string())
    }

    fn read_yaml(&self) -> Result<serde_yaml_ng::Value> {
        let content = fs::read_to_string(&self.path).context("Failed to read example.yaml")?;
        serde_yaml_ng::from_str(&content).context("Failed to parse example.yaml")
    }

    fn write_yaml(&self, doc: &serde_yaml_ng::Value) -> Result<()> {
        let content = serde_yaml_ng::to_string(doc).context("Failed to serialize updated config")?;
        fs::write(&self.path, content).context("Failed to write updated example.yaml")
    }

    fn apply_project_name(doc: &mut serde_yaml_ng::Value, name: &str) {
        if let Some(espforge) = doc.get_mut("espforge") {
            espforge["name"] = serde_yaml_ng::Value::String(name.to_string());
        }
    }

    fn apply_chip(doc: &mut serde_yaml_ng::Value, chip: &str) {
        if let Some(espforge) = doc.get_mut("espforge") {
            espforge["platform"] = serde_yaml_ng::Value::String(chip.to_string());
        }
    }
}

use anyhow::{Context, Result, bail};
use crate::cli::interactive::Prompter;
use crate::cli::model::ExampleConfig;
use std::fs;
use std::path::{Path, PathBuf};

pub struct OutputDirectory {
    path: PathBuf,
}

impl OutputDirectory {
    pub fn prepare(config: &ExampleConfig, prompter: &dyn Prompter) -> Result<Self> {
        let path = Self::resolve_path(&config.project_name)?;

        if path.exists() {
            Self::handle_existing_directory(&path, &config.project_name, prompter)?;
        } else {
            Self::create_directory(&path)?;
        }

        Ok(Self { path })
    }

    fn resolve_path(project_name: &str) -> Result<PathBuf> {
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        Ok(current_dir.join(project_name))
    }

    fn handle_existing_directory(
        path: &Path,
        project_name: &str,
        prompter: &dyn Prompter,
    ) -> Result<()> {
        let overwrite = prompter.confirm_overwrite(project_name)?;
        if !overwrite {
            bail!("Operation cancelled by user");
        }
        fs::remove_dir_all(path).context("Failed to remove existing directory")?;
        Ok(())
    }

    fn create_directory(path: &Path) -> Result<()> {
        fs::create_dir_all(path).context("Failed to create output directory")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
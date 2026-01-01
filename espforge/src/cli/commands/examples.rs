use crate::cli::interactive::{self, Prompter};
use crate::cli::model::{ExampleConfig, ExportResult};
use anyhow::{Context, Result, bail};
use espforge_examples::EXAMPLES_DIR;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ExamplesArgs {
    pub name: String,
    pub project_name: Option<String>,
    pub chip: Option<String>,
}

pub fn execute(args: ExamplesArgs) -> Result<()> {
    let prompter = interactive::DialoguerPrompter::new();
    execute_with_prompter(args, &prompter)
}

fn execute_with_prompter(args: ExamplesArgs, prompter: &dyn Prompter) -> Result<()> {
    let config = ConfigBuilder::from_args(args, prompter)?.build()?;
    let output = OutputDirectory::prepare(&config, prompter)?;
    let exporter = ExampleExporter::new();
    let result = exporter.export(&config, &output)?;

    ResultPrinter::display_success(&result);

    Ok(())
}

struct ConfigBuilder<'a> {
    args: ExamplesArgs,
    prompter: &'a dyn Prompter,
}

impl<'a> ConfigBuilder<'a> {
    fn from_args(args: ExamplesArgs, prompter: &'a dyn Prompter) -> Result<Self> {
        Ok(Self { args, prompter })
    }

    fn build(self) -> Result<ExampleConfig> {
        let name = self.resolve_example_name()?;
        let project_name = self.resolve_project_name(&name)?;
        let chip = self.resolve_chip()?;

        Ok(ExampleConfig {
            template_name: name,
            project_name,
            chip,
        })
    }

    fn resolve_example_name(&self) -> Result<String> {
        if self.args.name.is_empty() {
            self.prompter.select_example()
        } else {
            Ok(self.args.name.clone())
        }
    }

    fn resolve_project_name(&self, default: &str) -> Result<String> {
        match &self.args.project_name {
            Some(name) => Ok(name.clone()),
            None => self.prompter.prompt_project_name(default),
        }
    }

    fn resolve_chip(&self) -> Result<String> {
        match &self.args.chip {
            Some(chip) => Ok(chip.clone()),
            None => self.prompter.select_chip(),
        }
    }
}

struct OutputDirectory {
    path: PathBuf,
}

impl OutputDirectory {
    fn prepare(config: &ExampleConfig, prompter: &dyn Prompter) -> Result<Self> {
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

    fn path(&self) -> &Path {
        &self.path
    }
}

struct ExampleExporter;

impl ExampleExporter {
    fn new() -> Self {
        Self
    }

    fn export(&self, config: &ExampleConfig, output: &OutputDirectory) -> Result<ExportResult> {
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
            .ok_or_else(|| anyhow::anyhow!("Example template '{}' not found", name))?;

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
    // First, create the directory itself
    let dir_path = dir.path();
    let relative_dir_path = dir_path
        .strip_prefix(root_prefix)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    
    let dest_dir = base_path.join(relative_dir_path);
    if !dest_dir.exists() {
        fs::create_dir_all(&dest_dir)?;
    }

    // Then extract files
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

    // Recursively handle subdirectories
    for subdir in dir.dirs() {
        extract_recursive(subdir, base_path, root_prefix)?;
    }

    Ok(())
}

struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    const DEFAULT_NAME: &'static str = "example.yaml";

    fn locate(base_path: &Path) -> Result<Self> {
        let path = base_path.join(Self::DEFAULT_NAME);

        if !path.exists() {
            bail!("The example template is missing the required 'example.yaml' file");
        }

        Ok(Self { path })
    }

    fn update(&self, config: &ExampleConfig) -> Result<()> {
        let mut yaml_doc = self.read_yaml()?;

        Self::apply_project_name(&mut yaml_doc, &config.project_name);
        Self::apply_chip(&mut yaml_doc, &config.chip);

        self.write_yaml(&yaml_doc)
    }

    fn rename_to(&self, project_name: &str) -> Result<String> {
        let new_filename = format!("{}.yaml", project_name);
        let new_path = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid config file path"))?
            .join(&new_filename);

        fs::rename(&self.path, &new_path).context("Failed to rename configuration file")?;

        Ok(project_name.to_string())
    }

    fn read_yaml(&self) -> Result<serde_yaml_ng::Value> {
        let content = fs::read_to_string(&self.path).context("Failed to read example.yaml")?;

        serde_yaml_ng::from_str(&content).context("Failed to parse example.yaml")
    }

    fn write_yaml(&self, doc: &serde_yaml_ng::Value) -> Result<()> {
        let content =
            serde_yaml_ng::to_string(doc).context("Failed to serialize updated config")?;

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

struct ResultPrinter;

impl ResultPrinter {
    fn display_success(result: &ExportResult) {
        println!(
            "\n✨ Success! Project initialized in '{}'",
            result.project_name
        );
        println!("To compile the project:");
        println!("  cd {}", result.project_name);
        println!("  espforge compile {}", result.output_file);
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_config_builder_with_all_args() {
//         use crate::cli::interactive::MockPrompter;

//         let args = ExamplesArgs {
//             name: "blinky".to_string(),
//             project_name: Some("my_project".to_string()),
//             chip: Some("esp32c3".to_string()),
//         };

//         let mock = MockPrompter::new();
//         let config = ConfigBuilder::from_args(args, &mock)
//             .unwrap()
//             .build()
//             .unwrap();

//         assert_eq!(config.template_name, "blinky");
//         assert_eq!(config.project_name, "my_project");
//         assert_eq!(config.chip, "esp32c3");
//     }

//     #[test]
//     fn test_config_builder_with_prompts() {
//         use crate::cli::interactive::MockPrompter;

//         let args = ExamplesArgs {
//             name: String::new(),
//             project_name: None,
//             chip: None,
//         };

//         let mock = MockPrompter::new()
//             .with_example("wifi")
//             .with_project_name("wifi_project")
//             .with_chip("esp32s3");

//         let config = ConfigBuilder::from_args(args, &mock)
//             .unwrap()
//             .build()
//             .unwrap();

//         assert_eq!(config.template_name, "wifi");
//         assert_eq!(config.project_name, "wifi_project");
//         assert_eq!(config.chip, "esp32s3");
//     }
// }

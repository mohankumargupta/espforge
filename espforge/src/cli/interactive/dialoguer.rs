use super::{
    Prompter,
    catalog::ExampleCatalog};
use anyhow::{Result, bail};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use espforge_dialogue::{Asker, EnumAsker};

pub struct DialoguerPrompter {
    theme: ColorfulTheme,
}

#[derive(Debug, Clone, EnumAsker)]
#[asker(prompt = "Select Target Chip")]
pub enum Chip {
    #[asker(label = "esp32c3")]
    Esp32c3,
    #[asker(label = "esp32c6")]
    Esp32c6,
    #[asker(label = "esp32s3")]
    Esp32s3,
    #[asker(label = "esp32s2")]
    Esp32s2,
    #[asker(label = "esp32")]
    Esp32,
    #[asker(label = "esp32h2")]
    Esp32h2,
}

impl std::fmt::Display for Chip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Chip::Esp32c3 => "esp32c3",
            Chip::Esp32c6 => "esp32c6",
            Chip::Esp32s3 => "esp32s3",
            Chip::Esp32s2 => "esp32s2",
            Chip::Esp32 => "esp32",
            Chip::Esp32h2 => "esp32h2",
        };
        write!(f, "{}", s)
    }
}

#[derive(Asker)]
struct OverwritePrompt {
    #[confirm]
    confirm: bool,
}

impl DialoguerPrompter {
    pub fn new() -> Self {
        Self {
            theme: ColorfulTheme::default(),
        }
    }

    pub fn select_from_list<T: ToString + std::fmt::Display>(
        &self,
        prompt: impl Into<String>,
        items: &[T],
    ) -> Result<usize> {
        let selection = Select::with_theme(&self.theme)
            .with_prompt(prompt.into())
            .default(0)
            .items(items)
            .interact()?;
        Ok(selection)
    }

    pub fn ensure_not_empty<T>(items: &[T], context: &str) -> Result<()> {
        if items.is_empty() {
            bail!("No {} found to select from", context);
        }
        Ok(())
    }
}

impl Default for DialoguerPrompter {
    fn default() -> Self {
        Self::new()
    }
}

impl Prompter for DialoguerPrompter {
    fn select_example(&self) -> Result<String> {
        let examples_by_category = ExampleCatalog::load();
        let category = examples_by_category.select_category(self)?;
        let example = examples_by_category.select_example_from_category(&category, self)?;
        Ok(example)
    }

    fn prompt_project_name(&self, default: &str) -> Result<String> {
        let input = Input::with_theme(&self.theme)
            .with_prompt("Project Name (Destination Folder)")
            .default(default.to_string())
            .interact_text()?;
        Ok(input)
    }

    fn select_chip(&self) -> Result<String> {
        let chip = Chip::ask();
        Ok(chip.to_string())
    }

    fn confirm_overwrite(&self, dir_name: &str) -> Result<bool> {
        let prompt_text = format!("Directory '{}' already exists. Overwrite?", dir_name);
        let prompt = OverwritePrompt::asker()
            .confirm(prompt_text)
            .finish();
        Ok(prompt.confirm)
    }
}
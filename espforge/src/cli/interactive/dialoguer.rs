use super::{
    Prompter,
    catalog::{ChipCatalog, ExampleCatalog},
};
use anyhow::{Result, bail};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};

pub struct DialoguerPrompter {
    theme: ColorfulTheme,
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
        let chips = ChipCatalog::available_chips();
        let selection = self.select_from_list("Select Target Chip", &chips)?;
        Ok(chips[selection].to_string())
    }

    fn confirm_overwrite(&self, dir_name: &str) -> Result<bool> {
        let confirm = Confirm::with_theme(&self.theme)
            .with_prompt(format!(
                "Directory '{}' already exists. Overwrite?",
                dir_name
            ))
            .default(false)
            .interact()?;
        Ok(confirm)
    }
}

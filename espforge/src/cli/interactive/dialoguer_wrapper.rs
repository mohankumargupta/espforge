use super::Prompter;
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use miette::{IntoDiagnostic, Result, bail};
use std::collections::HashMap;

pub struct DialoguerPrompter {
    theme: ColorfulTheme,
}

impl DialoguerPrompter {
    pub fn new() -> Self {
        Self {
            theme: ColorfulTheme::default(),
        }
    }
}

impl Default for DialoguerPrompter {
    fn default() -> Self {
        Self::new()
    }
}

impl Prompter for DialoguerPrompter {
    fn select_example(&self) -> Result<String> {
        let categories_map = list_examples_by_category();
        let mut category_names: Vec<_> = categories_map.keys().collect();
        category_names.sort();

        if category_names.is_empty() {
            bail!("No examples found to select from.");
        }

        // Select category
        let cat_selection = Select::with_theme(&self.theme)
            .with_prompt("Select a Category")
            .default(0)
            .items(&category_names)
            .interact()
            .into_diagnostic()?;

        let selected_cat_name = category_names[cat_selection];
        let examples = &categories_map[selected_cat_name];

        if examples.is_empty() {
            bail!("No examples found in category '{}'", selected_cat_name);
        }

        // Select example
        let ex_selection = Select::with_theme(&self.theme)
            .with_prompt(format!("Select example from {}", selected_cat_name))
            .default(0)
            .items(examples)
            .interact()
            .into_diagnostic()?;

        Ok(examples[ex_selection].clone())
    }

    fn prompt_project_name(&self, default: &str) -> Result<String> {
        Input::with_theme(&self.theme)
            .with_prompt("Project Name (Destination Folder)")
            .default(default.to_string())
            .interact_text()
            .into_diagnostic()
    }

    fn select_chip(&self) -> Result<String> {
        let chips = vec![
            "esp32c3", "esp32c6", "esp32s3", "esp32s2", "esp32", "esp32h2",
        ];

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select Target Chip")
            .default(0)
            .items(&chips)
            .interact()
            .into_diagnostic()?;

        Ok(chips[selection].to_string())
    }

    fn confirm_overwrite(&self, dir_name: &str) -> Result<bool> {
        Confirm::with_theme(&self.theme)
            .with_prompt(format!(
                "Directory '{}' already exists. Overwrite?",
                dir_name
            ))
            .default(false)
            .interact()
            .into_diagnostic()
    }
}

fn list_examples_by_category() -> HashMap<String, Vec<String>> {
    use espforge_examples::EXAMPLES_DIR;
    let mut map = HashMap::new();

    for entry in EXAMPLES_DIR.dirs() {
        let category = entry
            .path()
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut examples = Vec::new();
        for example_entry in entry.dirs() {
            let example = example_entry
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            examples.push(example);
        }
        if !examples.is_empty() {
            map.insert(category, examples);
        }
    }
    map
}

// ============================================================================
// Example: Mock implementation for testing
// ============================================================================
#[cfg(test)]
pub struct MockPrompter {
    pub example_name: String,
    pub project_name: String,
    pub chip: String,
    pub allow_overwrite: bool,
}

#[cfg(test)]
impl Prompter for MockPrompter {
    fn select_example(&self) -> Result<String> {
        Ok(self.example_name.clone())
    }

    fn prompt_project_name(&self, _default: &str) -> Result<String> {
        Ok(self.project_name.clone())
    }

    fn select_chip(&self) -> Result<String> {
        Ok(self.chip.clone())
    }

    fn confirm_overwrite(&self, _dir_name: &str) -> Result<bool> {
        Ok(self.allow_overwrite)
    }
}

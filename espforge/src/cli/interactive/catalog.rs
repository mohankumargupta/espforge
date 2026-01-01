use anyhow::Result;
use std::collections::HashMap;

use crate::cli::interactive::DialoguerPrompter;

pub struct ExampleCatalog {
    categories: HashMap<String, Vec<String>>,
}

impl ExampleCatalog {
    pub fn load() -> Self {
        use espforge_examples::EXAMPLES_DIR;

        let categories = EXAMPLES_DIR
            .dirs()
            .filter_map(|entry| {
                let category = Self::extract_name(entry.path().file_name());
                let examples: Vec<String> = entry
                    .dirs()
                    .map(|ex| Self::extract_name(ex.path().file_name()))
                    .collect();

                if examples.is_empty() {
                    None
                } else {
                    Some((category, examples))
                }
            })
            .collect();

        Self { categories }
    }

    fn extract_name(file_name: Option<&std::ffi::OsStr>) -> String {
        file_name.unwrap_or_default().to_string_lossy().to_string()
    }

    pub fn select_category(&self, prompter: &DialoguerPrompter) -> Result<String> {
        let mut category_names: Vec<_> = self.categories.keys().cloned().collect();
        category_names.sort();

        DialoguerPrompter::ensure_not_empty(&category_names, "example categories")?;

        let selection = prompter.select_from_list("Select a Category", &category_names)?;
        Ok(category_names[selection].clone())
    }

    pub fn select_example_from_category(
        &self,
        category: &str,
        prompter: &DialoguerPrompter,
    ) -> Result<String> {
        let examples = self
            .categories
            .get(category)
            .ok_or_else(|| anyhow::anyhow!("Category '{}' not found", category))?;

        DialoguerPrompter::ensure_not_empty(examples, &format!("examples in '{}'", category))?;

        let selection =
            prompter.select_from_list(format!("Select example from {}", category), examples)?;

        Ok(examples[selection].clone())
    }

    pub fn categories(&self) -> Vec<&String> {
        let mut cats: Vec<_> = self.categories.keys().collect();
        cats.sort();
        cats
    }

    pub fn examples_in_category(&self, category: &str) -> Option<&Vec<String>> {
        self.categories.get(category)
    }
}

pub struct ChipCatalog;

impl ChipCatalog {
    pub fn available_chips() -> Vec<&'static str> {
        vec![
            "esp32c3", "esp32c6", "esp32s3", "esp32s2", "esp32", "esp32h2",
        ]
    }

    pub fn is_valid_chip(chip: &str) -> bool {
        Self::available_chips().contains(&chip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chip_validation() {
        assert!(ChipCatalog::is_valid_chip("esp32c3"));
        assert!(ChipCatalog::is_valid_chip("esp32"));
        assert!(!ChipCatalog::is_valid_chip("esp32c9"));
        assert!(!ChipCatalog::is_valid_chip("invalid"));
    }
}

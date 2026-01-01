//! Mock prompter for testing

use super::Prompter;
use anyhow::Result;

/// Mock implementation for testing without user interaction
/// 
/// Useful for unit tests and CI environments where interactive
/// prompts would block execution.
pub struct MockPrompter {
    pub example_name: String,
    pub project_name: String,
    pub chip: String,
    pub allow_overwrite: bool,
}

impl MockPrompter {
    /// Create a new mock prompter with default values
    pub fn new() -> Self {
        Self {
            example_name: "default_example".to_string(),
            project_name: "test_project".to_string(),
            chip: "esp32c3".to_string(),
            allow_overwrite: false,
        }
    }

    /// Builder method to set example name
    pub fn with_example(mut self, name: impl Into<String>) -> Self {
        self.example_name = name.into();
        self
    }

    /// Builder method to set project name
    pub fn with_project_name(mut self, name: impl Into<String>) -> Self {
        self.project_name = name.into();
        self
    }

    /// Builder method to set chip
    pub fn with_chip(mut self, chip: impl Into<String>) -> Self {
        self.chip = chip.into();
        self
    }

    /// Builder method to set overwrite behavior
    pub fn with_overwrite(mut self, allow: bool) -> Self {
        self.allow_overwrite = allow;
        self
    }
}

impl Default for MockPrompter {
    fn default() -> Self {
        Self::new()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_prompter_builder() {
        let mock = MockPrompter::new()
            .with_example("blinky")
            .with_project_name("my_project")
            .with_chip("esp32s3")
            .with_overwrite(true);

        assert_eq!(mock.select_example().unwrap(), "blinky");
        assert_eq!(mock.prompt_project_name("ignored").unwrap(), "my_project");
        assert_eq!(mock.select_chip().unwrap(), "esp32s3");
        assert!(mock.confirm_overwrite("any_dir").unwrap());
    }
}
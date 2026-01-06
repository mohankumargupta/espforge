use anyhow::Result;
use crate::cli::interactive::Prompter;
use crate::cli::model::ExampleConfig;
use super::ExamplesArgs;

pub struct ConfigBuilder<'a> {
    args: ExamplesArgs,
    prompter: &'a dyn Prompter,
}

impl<'a> ConfigBuilder<'a> {
    pub fn from_args(args: ExamplesArgs, prompter: &'a dyn Prompter) -> Result<Self> {
        Ok(Self { args, prompter })
    }

    pub fn build(self) -> Result<ExampleConfig> {
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


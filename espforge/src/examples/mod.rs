pub mod builder;
pub mod config;
pub mod fs;
pub mod template;
pub mod ui;

use anyhow::Result;
use crate::cli::interactive::{self, Prompter};
use builder::ConfigBuilder;
use fs::OutputDirectory;
use template::ExampleExporter;
use ui::ResultPrinter;

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
    // 1. Resolve Configuration (Args + User Input)
    let config = ConfigBuilder::from_args(args, prompter)?.build()?;

    // 2. Prepare Output Directory (Check existence, confirm overwrite)
    let output = OutputDirectory::prepare(&config, prompter)?;

    // 3. Export the Template and Update Config
    let exporter = ExampleExporter::new();
    let result = exporter.export(&config, &output)?;

    // 4. Display Success
    ResultPrinter::display_success(&result);

    Ok(())
}


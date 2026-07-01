pub mod builder;
pub mod config;
pub mod fs;
pub mod template;
pub mod ui;

use crate::cli::interactive::{self, Prompter};
use crate::cli::model::ExampleConfig;
use anyhow::Result;
use builder::ConfigBuilder;
use fs::OutputDirectory;
use template::{ExampleExporter, ExampleTemplate};
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

pub fn execute_noninteractive(args: ExamplesArgs) -> Result<()> {
    let template_name = args.name.clone();
    let project_name = args.project_name.unwrap_or_else(|| args.name.clone());

    // Resolve chip: use --chip arg > read from example.yaml > default to esp32c3
    let chip = if let Some(chip) = args.chip {
        chip
    } else {
        ExampleTemplate::find(&template_name)
            .ok()
            .and_then(|t| t.read_chip_from_yaml())
            .unwrap_or_else(|| "esp32c3".to_string())
    };

    let config = ExampleConfig {
        template_name,
        project_name,
        chip,
    };

    // Validate example exists BEFORE creating any directories
    ExampleTemplate::find(&config.template_name)
        .map_err(|_| anyhow::anyhow!("Example '{}' not found", config.template_name))?;

    // 2. Prepare Output Directory (Check existence, fail if exists)
    let output = OutputDirectory::prepare_noninteractive(&config)?;

    // 3. Export the Template and Update Config
    let exporter = ExampleExporter::new();
    let result = exporter.export(&config, &output)?;

    // 4. Display Success
    ResultPrinter::display_success(&result);

    Ok(())
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

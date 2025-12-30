use crate::cli::interactive::{self, Prompter};
use crate::cli::model::{ExampleConfig, ExportOptions, ExportResult};
use miette::{Context, IntoDiagnostic, Result, bail};
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

// Separate function for easier testing
fn execute_with_prompter(args: ExamplesArgs, prompter: &dyn Prompter) -> Result<()> {
    // 1. Gather configuration (with prompts as needed)
    let config = gather_config(args, prompter)?;

    // 2. Validate and prepare
    let target_path = prepare_output_directory(&config, prompter)?;

    // 3. Execute export
    let result = export_project(&config, &target_path)?;

    // 4. Display success message
    display_success(&result);

    Ok(())
}

fn gather_config(args: ExamplesArgs, prompter: &dyn Prompter) -> Result<ExampleConfig> {
    let name = if args.name.is_empty() {
        prompter.select_example()?
    } else {
        args.name
    };

    let project_name = match args.project_name {
        Some(n) => n,
        None => prompter.prompt_project_name(&name)?,
    };

    let chip = match args.chip {
        Some(c) => c,
        None => prompter.select_chip()?,
    };

    Ok(ExampleConfig {
        template_name: name,
        project_name,
        chip,
    })
}

fn prepare_output_directory(config: &ExampleConfig, prompter: &dyn Prompter) -> Result<PathBuf> {
    let current_dir = std::env::current_dir()
        .into_diagnostic()
        .context("Failed to get current directory")?;
    let target_path = current_dir.join(&config.project_name);

    if target_path.exists() {
        let overwrite = prompter.confirm_overwrite(&config.project_name)?;
        if !overwrite {
            bail!("Operation cancelled by user");
        }
    } else {
        std::fs::create_dir_all(&target_path)
            .into_diagnostic()
            .context("Failed to create output directory")?;
    }

    Ok(target_path)
}

fn export_project(config: &ExampleConfig, target_path: &PathBuf) -> Result<ExportResult> {
    let options = ExportOptions {
        example_name: config.template_name.clone(),
        override_project_name: Some(config.project_name.clone()),
        override_platform: Some(config.chip.clone()),
    };

    let exported_name = export_example(options, target_path).context("Failed to export example")?;

    Ok(ExportResult {
        project_name: config.project_name.clone(),
        output_file: format!("{}.yaml", exported_name),
    })
}

fn display_success(result: &ExportResult) {
    println!(
        "\n✨ Success! Project initialized in '{}'",
        result.project_name
    );
    println!("To compile the project:");
    println!("  cd {}", result.project_name);
    println!("  espforge compile {}", result.output_file);
}

fn export_example(_options: ExportOptions, _target_path: &Path) -> Result<String> {
    // Stub implementation for export
    // In a real implementation, this would copy files from EXAMPLES_DIR to target_path
    // and return the name of the main config file.
    Ok("example".to_string())
}

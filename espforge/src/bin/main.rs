use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use espforge_lib::compile;
use espforge_lib::export;
use espforge_lib::template_utils::list_examples_by_category;
use std::{fs::metadata, path::PathBuf};
// Import dialoguer traits
use dialoguer::{Input, Select, theme::ColorfulTheme};

#[derive(Parser)]
#[command(about = "Example tool with a compile subcommand")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compile {
        file: PathBuf,
        #[arg(long, short = 'c')]
        chip: Option<String>,
    },
    Examples {
        /// The name of the example template to use (e.g. blink, uart).
        /// If not provided, an interactive selection menu will be shown.
        #[arg(default_value = "")]
        name: String,

        #[arg(long, short = 'n')]
        project_name: Option<String>,

        /// Override the platform/chip (e.g. esp32s3)
        #[arg(long, short = 'c')]
        chip: Option<String>,
    },
}

pub fn main() -> Result<(), Error> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compile { file, chip: _ } => {
            metadata(&file)
                .with_context(|| format!("Configuration file {} not found", &file.display()))?;
            if !file.is_file() {
                anyhow::bail!("Path {} is not a file", file.display());
            }
            compile::compile(&file)?;
            Ok(())
        }

        Commands::Examples {
            mut name,
            mut project_name,
            mut chip,
        } => {
            let theme = ColorfulTheme::default();

            // 1. Interactive Template Selection (if name not provided)
            if name.is_empty() {
                // Get the map of categories -> examples
                let categories_map = list_examples_by_category();

                // Extract just the category names (keys)
                let mut category_names: Vec<_> = categories_map.keys().collect();
                category_names.sort();

                if category_names.is_empty() {
                    anyhow::bail!("No examples found to select from.");
                }

                // Step A: Select Category
                let cat_selection = Select::with_theme(&theme)
                    .with_prompt("Select a Category")
                    .default(0)
                    .items(&category_names)
                    .interact()?;

                let selected_cat_name = category_names[cat_selection];
                let examples = &categories_map[selected_cat_name];

                if examples.is_empty() {
                    anyhow::bail!("No examples found in category '{}'", selected_cat_name);
                }

                // Step B: Select Example within Category
                let ex_selection = Select::with_theme(&theme)
                    .with_prompt(format!("Select example from {}", selected_cat_name))
                    .default(0)
                    .items(examples)
                    .interact()?;

                name = examples[ex_selection].clone();
            }

            // 2. Interactive Project Name / Folder (if not provided)
            if project_name.is_none() {
                let input: String = Input::with_theme(&theme)
                    .with_prompt("Project Name (Destination Folder)")
                    .default(name.clone())
                    .interact_text()?;
                project_name = Some(input);
            }

            // 3. Interactive Chip Selection (if not provided)
            if chip.is_none() {
                let chips = vec![
                    "esp32c3", "esp32c6", "esp32s3", "esp32s2", "esp32", "esp32h2",
                ];

                let selection = Select::with_theme(&theme)
                    .with_prompt("Select Target Chip")
                    .default(0) // Default to C3
                    .items(&chips)
                    .interact()?;

                chip = Some(chips[selection].to_string());
            }

            let current_dir = std::env::current_dir()?;
            // Use the provided project name or the template name as the directory name
            let final_name = project_name.clone().unwrap_or_else(|| name.clone());

            let output_folder = final_name.clone();
            let target_path = current_dir.join(&output_folder);

            if target_path.exists() {
                // Optional: Ask to overwrite?
                let overwrite = dialoguer::Confirm::with_theme(&theme)
                    .with_prompt(format!(
                        "Directory '{}' already exists. Overwrite?",
                        output_folder
                    ))
                    .default(false)
                    .interact()?;

                if !overwrite {
                    println!("Aborting.");
                    return Ok(());
                }
            } else {
                // Create the directory
                std::fs::create_dir_all(&target_path)?;
            }

            let options = export::ExportOptions {
                example_name: name.clone(),
                override_project_name: project_name,
                override_platform: chip,
            };

            // Call the function in the main lib
            let exported_name = export::export_example(options, &target_path)?;

            let output_file = format!("{}.yaml", exported_name);

            println!("\n✨ Success! Project initialized in '{}'", output_folder);
            println!("To compile the project:");
            println!("  cd {}", output_folder);
            println!("  espforge compile {}", output_file);

            Ok(())
        }
    }
}

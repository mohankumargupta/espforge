use clap::{Parser, Subcommand, command};
use dialoguer::{Select, theme::ColorfulTheme};
use espforge_examples::EXAMPLES_DIR;

#[derive(Parser)]
#[command(about = "Example tool with a compile subcommand")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Examples {
        #[arg(default_value = "")]
        name: String,

        #[arg(long, short = 'n')]
        project_name: Option<String>,

        /// Override the platform/chip (e.g. esp32s3)
        #[arg(long, short = 'c')]
        chip: Option<String>,
    },
}

pub fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Examples {
            name,
            project_name,
            chip,
        } => {
            if name.is_empty() {
                interactive_dialog()?;
            } else if let (Some(pn), Some(c)) = (&project_name, &chip) {
                println!("{} {} {}", name, pn, c);
            }
        }
    }
    Ok(())
}

fn interactive_dialog() -> miette::Result<()> {
    // 1. List Categories (01.Basics, etc)
    let mut categories: Vec<_> = EXAMPLES_DIR
        .dirs()
        .filter_map(|d| d.path().file_name()?.to_str())
        .collect();
    
    categories.sort();

    if categories.is_empty() {
        // If this hits, the EXAMPLES_DIR is empty or path resolution failed silently
        println!("No example categories found in the embedded library.");
        return Ok(());
    }

    // 2. Select Category
    let category_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a category")
        .default(0)
        .items(&categories)
        .interact()
        .map_err(|e| miette::miette!("Dialog failed: {}", e))?;

    let selected_cat = categories[category_idx];

    // 3. List Examples
    let cat_dir = EXAMPLES_DIR
        .get_dir(selected_cat)
        .ok_or_else(|| miette::miette!("Failed to load category directory"))?;

    let mut examples: Vec<_> = cat_dir
        .dirs()
        .filter_map(|d| d.path().file_name()?.to_str())
        .collect();

    examples.sort();

    // 4. Select Example
    let example_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select an example")
        .default(0)
        .items(&examples)
        .interact()
        .map_err(|e| miette::miette!("Dialog failed: {}", e))?;

    let selected_ex = examples[example_idx];

    println!("\nYou selected: {}/{}", selected_cat, selected_ex);
    println!("Run: espforge examples {}/{}\n", selected_cat, selected_ex);

    Ok(())
}

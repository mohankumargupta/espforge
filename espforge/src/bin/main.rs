use clap::{Parser, Subcommand, command};

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
                interactive_dialog();
            } else {
                if let (Some(pn), Some(c)) = (&project_name, &chip) {
                    println!("{} {} {}", name, pn, c);
                }
            }
        }
    }
    Ok(())
}

fn interactive_dialog() {}

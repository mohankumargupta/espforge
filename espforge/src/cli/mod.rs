use anyhow::Result;
use clap::{Parser, Subcommand, command};
use std::path::PathBuf;

pub mod commands;
pub mod interactive;
pub mod model;

use commands::compile;
use commands::examples;

#[derive(Parser)]
#[command(about = "Example tool with a compile subcommand")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compile {
        file: PathBuf,
    },
    Examples {
        #[arg(default_value = "")]
        name: String,
        #[arg(long, short = 'n')]
        project_name: Option<String>,
        #[arg(long, short = 'c')]
        chip: Option<String>,
    },
}

impl Cli {
    pub fn execute(self) -> Result<()> {
        match self.command {
            Commands::Compile { file } => compile::execute(&file),
            Commands::Examples {
                name,
                project_name,
                chip,
            } => examples::execute(examples::ExamplesArgs {
                name,
                project_name,
                chip,
            }),
        }
    }
}

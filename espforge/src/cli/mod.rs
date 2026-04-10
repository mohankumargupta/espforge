use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod commands;
pub mod interactive;
pub mod model;

use commands::{compile, doctor, examples};
use crate::examples::ExamplesArgs;
use crate::examples::execute_noninteractive;

#[derive(Parser)]
#[command(version, about = "Example tool with a compile subcommand")]
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
    Example {
        name: String,
        #[arg(long, short = 'n')]
        project_name: Option<String>,
        #[arg(long, short = 'c')]
        chip: Option<String>,
    },
    Doctor,
}

impl Cli {
    pub fn execute(self) -> Result<()> {
        match self.command {
            Commands::Compile { file } => compile::execute(&file),
            Commands::Examples {
                name,
                project_name,
                chip,
            } => examples::execute(ExamplesArgs {
                name,
                project_name,
                chip,
            }),
            Commands::Example {
                name,
                project_name,
                chip,
            } => execute_noninteractive(ExamplesArgs {
                name,
                project_name,
                chip,
            }),    
            Commands::Doctor => doctor::execute(),
        }
    }
}

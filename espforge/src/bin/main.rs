use anyhow::Result;
use clap::Parser;
use espforge_lib::cli;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.execute()
}

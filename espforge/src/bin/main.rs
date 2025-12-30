use clap::Parser;
use espforge_lib::cli;

fn main() -> miette::Result<()> {
    let cli = cli::Cli::parse();
    cli.execute()
}

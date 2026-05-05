use anyhow::Result;
use clap::Parser;
use espforge_lib::cli;

fn main() -> Result<()> {
    use espforge_components_builder;
    use espforge_devices_builder;

    // hack to ensure plugins are linked ins
    // let _ = espforge_components_builder::button::ButtonPlugin;
    // let _ = espforge_devices_builder::ili9341::ILI9341Plugin;
    espforge_components_builder::init();
    espforge_devices_builder::init();

    let cli = cli::Cli::parse();
    cli.execute()
}

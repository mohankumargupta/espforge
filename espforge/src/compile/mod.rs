use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use std::fs;
use std::path::Path;

use crate::parse::ConfigParser;

mod assets;
mod dependencies;
mod generators;

pub fn compile_project(config_path: &Path) -> Result<()> {
    let compiler = ProjectCompiler::new(config_path)?;
    compiler.run()
}

struct ProjectCompiler {
    model: EspforgeConfiguration,
}

impl ProjectCompiler {
    fn new(config_path: &Path) -> Result<Self> {
        println!("🔍 Parsing configuration...");
        let content = fs::read_to_string(config_path).context(format!(
            "Failed to read configuration file: {}",
            config_path.display()
        ))?;

        let parser = ConfigParser::new();
        let model = parser.parse(&content)?;

        Ok(Self { model })
    }

    fn run(&self) -> Result<()> {
        println!("   Project: {}", self.model.get_name());
        println!("   Chip:    {}", self.model.get_chip());
        println!("   Runtime: {}", self.model.runtime_name());
        println!("🔨 Generating artifacts...");

        let project_dir = std::env::current_dir().context("Failed to get current directory")?;
        let src_dir = project_dir.join("src");

        assets::copy_app_rust_to_src(&project_dir, &src_dir)?;

        generators::generate_scaffold(&self.model, &project_dir)?;

        assets::setup_wifi_env_config(&project_dir, &self.model)?;

        let additional_modules = assets::inject_app_code(&src_dir)?;
        generators::setup_library_structure(&src_dir, &additional_modules, &self.model)?;
        generators::generate_component_code(&src_dir, &self.model)?;
        generators::generate_entry_point(&src_dir, &self.model)?;

        dependencies::add_dependencies(&project_dir, &self.model)?;
        assets::generate_wokwi_config(&project_dir, &self.model)?;
        assets::copy_wokwi_files(&project_dir)?;

        println!("✨ Rust project generated successfully!");
        println!();
        println!("To build run: cargo build");

        Ok(())
    }

    // fn resolve_project_dir(&self) -> Result<PathBuf> {
    //     Ok(self.base_dir.clone())
    // }
}

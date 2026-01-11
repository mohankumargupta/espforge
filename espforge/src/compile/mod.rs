use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::parse::ConfigurationOrchestrator;
use crate::parse::model::EspforgeConfiguration;

// Declare sub-modules
mod assets;
mod dependencies;
mod generators;

pub fn compile_project(config_path: &Path) -> Result<()> {
    let compiler = ProjectCompiler::new(config_path)?;
    compiler.run()
}

struct ProjectCompiler {
    base_dir: PathBuf,
    model: EspforgeConfiguration,
}

impl ProjectCompiler {
    fn new(config_path: &Path) -> Result<Self> {
        println!("🔍 Parsing configuration...");
        let content = fs::read_to_string(config_path).context(format!(
            "Failed to read configuration file: {}",
            config_path.display()
        ))?;

        let orchestrator = ConfigurationOrchestrator::new();
        let model = orchestrator.compile(&content)?;

        let base_dir = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        Ok(Self { base_dir, model })
    }

    fn run(&self) -> Result<()> {
        println!("   Project: {}", self.model.get_name());
        println!("   Chip:    {}", self.model.get_chip());

        println!("🔨 Generating artifacts...");
        
        // Step 1: Scaffold
        generators::generate_scaffold(&self.model)?;
        
        let project_dir = self.resolve_project_dir()?;
        let src_dir = project_dir.join("src");

        // Step 2: Dependencies
        dependencies::add_dependencies(&project_dir)?;
        
        // Step 3: Assets (Wokwi, Platform, User App)
        assets::copy_wokwi_files(&self.base_dir, &project_dir)?;
        assets::provision_platform_assets(&project_dir, &src_dir)?;
        assets::inject_app_code(&self.base_dir, &src_dir)?;
        
        // Step 4: Code Generation
        generators::generate_component_code(&src_dir, &self.model)?;
        generators::setup_library_structure(&src_dir)?;
        generators::generate_entry_point(&src_dir, &self.model)?;

        println!("✨ Project compiled successfully!");
        Ok(())
    }

    fn resolve_project_dir(&self) -> Result<PathBuf> {
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        Ok(current_dir.join(self.model.get_name()))
    }
}


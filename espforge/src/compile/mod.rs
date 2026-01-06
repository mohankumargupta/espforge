use anyhow::{Context, Result, anyhow};
use quote::quote;
use std::fs;
use std::path::{Path, PathBuf};

use crate::codegen::espgenerate::esp_generate;
use crate::parse::ConfigurationOrchestrator;
use crate::parse::model::ProjectModel;

pub fn compile_project(config_path: &Path) -> Result<()> {
    let compiler = ProjectCompiler::new(config_path)?;
    compiler.run()
}

struct ProjectCompiler {
    base_dir: PathBuf,
    model: ProjectModel,
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
        self.generate_scaffold()?;
        
        self.add_dependencies()?;
        let project_dir = self.resolve_project_dir()?;
        let src_dir = project_dir.join("src");
        self.copy_wokwi_files(&project_dir)?;
        self.provision_platform_assets(&project_dir, &src_dir)?;
        self.generate_component_code(&src_dir)?;
        self.setup_library_structure(&src_dir)?;
        self.inject_app_code(&src_dir)?;
        self.generate_entry_point(&src_dir)?;

        println!("✨ Project compiled successfully!");
        Ok(())
    }

    fn add_dependencies(&self) -> Result<()> {
        let cargo_path = self.resolve_project_dir()?.join("Cargo.toml");
        let manifest = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;
        let mut doc = manifest.parse::<toml_edit::DocumentMut>().context("Failed to parse Cargo.toml")?;
        
        let reference_toml = espforge_examples::EXTRA_DEPENDENCIES
            .parse::<toml_edit::DocumentMut>()
            .context("Failed to parse reference dependencies.toml")?;

        // Extract the [dependencies] table from the reference
        let source_deps = reference_toml
            .get("dependencies")
            .and_then(|item| item.as_table())
            .ok_or_else(|| anyhow!("dependencies.toml is missing the [dependencies] section"))?;

        // Merge into the target [dependencies]
        if let Some(target_deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
            for (dep_name, dep_value) in source_deps {
                // Only add if not already present (prevents overwriting user customizations if they exist)
                if !target_deps.contains_key(dep_name) {
                    target_deps[dep_name] = dep_value.clone();
                }
            }
         }        

        // if let Some(deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) {
        //     if !deps.contains_key("embedded-hal") {
        //         deps["embedded-hal"] = toml_edit::value("1.0.0");
        //     }
        //     if !deps.contains_key("embedded-hal-bus") {
        //         deps["embedded-hal-bus"] = toml_edit::value("0.3.0");
        //     }
        //     if !deps.contains_key("static_cell") {
        //     deps["static_cell"] = toml_edit::value("2.1");
        // }
        // }
        
        fs::write(cargo_path, doc.to_string()).context("Failed to write Cargo.toml")?;
        Ok(())
    }


    fn resolve_project_dir(&self) -> Result<PathBuf> {
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;
        Ok(current_dir.join(self.model.get_name()))
    }

    fn generate_scaffold(&self) -> Result<()> {
        esp_generate(self.model.get_name(), self.model.get_chip(), false)
    }

    fn copy_wokwi_files(&self, project_dir: &Path) -> Result<()> {
        let files_to_check = ["diagram.json", "wokwi.toml"];
        for filename in files_to_check {
            let source_path = self.base_dir.join(filename);
            if source_path.exists() {
                let dest_path = project_dir.join(filename);
                fs::copy(&source_path, &dest_path)
                    .with_context(|| format!("Failed to copy {} to project", filename))?;
                println!("   Overriding generated {} with custom file", filename);
            }
        }
        Ok(())
    }

    fn provision_platform_assets(&self, project_dir: &Path, src_dir: &Path) -> Result<()> {
        let platform_temp = project_dir.join(".espforge_temp");

        if platform_temp.exists() {
            fs::remove_dir_all(&platform_temp)?;
        }
        fs::create_dir_all(&platform_temp)?;

        crate::PLATFORM_SRC
            .extract(&platform_temp)
            .context("Failed to extract platform assets")?;

        let assets_dir = platform_temp.join("assets");
        let platform_dest = src_dir.join("platform");

        if assets_dir.exists() {
            copy_recursive(&assets_dir, &platform_dest)?;
        }

        let _ = fs::remove_dir_all(platform_temp);
        Ok(())
    }

    fn generate_component_code(&self, src_dir: &Path) -> Result<()> {
        let components_src = crate::codegen::components::generate_components_source(&self.model)?;
        fs::write(src_dir.join("generated.rs"), components_src)
            .context("Failed to write src/generated.rs")?;
        Ok(())
    }

    fn setup_library_structure(&self, src_dir: &Path) -> Result<()> {
        let tokens = quote! {
            #![no_std]
            pub mod app;
            pub mod platform;
            pub mod generated;

            pub use platform::*;

            pub struct Context {
                pub logger: platform::logger::Logger,
                pub delay: platform::delay::Delay,
                pub components: generated::Components<'static>,
            }
        };
        fs::write(src_dir.join("lib.rs"), tokens.to_string())
            .context("Failed to write src/lib.rs")?;
        Ok(())
    }

    fn inject_app_code(&self, src_dir: &Path) -> Result<()> {
        let rust_source = self.base_dir.join("app/rust/app.rs");
        let target = src_dir.join("app.rs");

        if rust_source.exists() {
            fs::copy(&rust_source, &target)?;
            println!("   Included app logic from app/rust/app.rs");
        } else {
            println!("⚠️  Warning: No app code found. Generating stub.");
            fs::write(
                &target,
                r#"
                // Stub generated by espforge
                pub fn setup(_: &mut crate::Context) {}
                pub fn forever(_: &mut crate::Context) {}
            "#,
            )?;
        }
        Ok(())
    }

    fn generate_entry_point(&self, src_dir: &Path) -> Result<()> {
        let crate_name = self.model.get_name().replace('-', "_");
        let content = espforge_templates::render_main(&crate_name)
            .map_err(|e| anyhow!("Failed to render main.rs: {}", e))?;

        let path = src_dir.join("bin/main.rs");
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(&path, content).context("Failed to write generated main.rs")?;
        Ok(())
    }
}

// Helper utility
fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_recursive(&entry.path(), &target_path)?;
        } else if entry.file_name() == "lib.rs" {
            // Rename internal lib.rs to mod.rs when moving to submodules
            fs::copy(entry.path(), dst.join("mod.rs"))?;
        } else {
            fs::copy(entry.path(), target_path)?;
        }
    }
    Ok(())
}
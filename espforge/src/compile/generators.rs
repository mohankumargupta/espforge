use crate::parse::model::EspforgeConfiguration;
use anyhow::{Context, Result, anyhow};
use espforge_codegen::{esp_generate, generate_components_source};
use quote::quote;
use std::fs;
use std::path::Path;

/// Invokes the esp-generate CLI tool to create the initial project structure
pub fn generate_scaffold(model: &EspforgeConfiguration) -> Result<()> {
    esp_generate(model.get_name(), model.get_chip(), false)
}

/// Generates the generated.rs file containing component definitions
pub fn generate_component_code(src_dir: &Path, model: &EspforgeConfiguration) -> Result<()> {
    let components_src = generate_components_source(model)?;
    fs::write(src_dir.join("generated.rs"), components_src)
        .context("Failed to write src/generated.rs")?;
    Ok(())
}

/// Creates the lib.rs file that exports the project structure
pub fn setup_library_structure(src_dir: &Path) -> Result<()> {
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

/// Renders and writes the main entry point (main.rs)
pub fn generate_entry_point(src_dir: &Path, model: &EspforgeConfiguration) -> Result<()> {
    let crate_name = model.get_name().replace('-', "_");
    let content = espforge_templates::render_main(&crate_name)
        .map_err(|e| anyhow!("Failed to render main.rs: {}", e))?;

    let path = src_dir.join("bin/main.rs");
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(&path, content).context("Failed to write generated main.rs")?;
    Ok(())
}

use anyhow::Result;
use espforge_configuration::EspforgeConfiguration;

pub mod allocators;
pub mod builders;
pub mod component_builders;
pub mod context;
pub mod dependency;
pub mod generator;
pub mod registry;
pub mod resolver;
pub mod scaffold;
pub mod templates;

pub use scaffold::esp_generate;

// Re-export main generation functions
pub use templates::{generate_components_source, generate_entry_point_source, generate_lib_source};

// Convenience function that maintains backward compatibility
pub fn generate_all(
    model: &EspforgeConfiguration,
    additional_modules: &[String],
) -> Result<(String, String, String)> {
    let lib_source = generate_lib_source(additional_modules, model)?;
    let entry_point = generate_entry_point_source(model)?;
    let components = generate_components_source(model)?;

    Ok((lib_source, entry_point, components))
}

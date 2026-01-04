use anyhow::Result;

use crate::parse::model::ProjectModel;

/// Resolves dependencies and validates the project model.
///
/// This step connects components to their definitions and validates 
/// that referenced GPIOs exist.
pub fn resolve_project(model: &mut ProjectModel) -> Result<()> {
    // Example logic:
    // 1. Check that all 'components' referring to '$gpioX' actually have a 
    //    corresponding entry in the 'esp32' (GPIO) section.
    
    for (comp_name, config) in &model.components {
        for (prop_key, prop_val) in &config.properties {
            if let Some(val_str) = prop_val.as_str() {
                if val_str.starts_with('$') {
                    let gpio_ref = &val_str[1..]; // remove '$'
                    if !model.gpios.contains_key(gpio_ref) {
                        println!("⚠️  Warning: Component '{}' references undefined GPIO '{}' in property '{}'", 
                            comp_name, gpio_ref, prop_key);
                        // In strict mode, we might return Err here.
                    }
                }
            }
        }
    }
    
    Ok(())
}



use crate::config::EspforgeConfiguration;
use crate::template_utils::{find_template_path, process_template_directory};
use anyhow::Result;
use std::path::Path;

pub fn apply_templates(config: &EspforgeConfiguration, project_path: &Path, context: &tera::Context) -> Result<()> {
    let project_str = project_path.to_str().expect("Valid UTF-8 project path");
    
    process_template_directory("_dynamic", project_str, context)?;

    if let Some(template_name) = config.get_template() {
        if let Some(path) = find_template_path(&template_name) {
            process_template_directory(&path, project_str, context)?;
        }
    }
    Ok(())
}

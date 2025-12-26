use crate::config::EspforgeConfiguration;
use crate::template_utils::{find_template_path, get_templates};
use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn load_ruchy_source(config: &EspforgeConfiguration, config_dir: &Path) -> Result<Option<String>> {
    let local_script = config_dir.join("app.ruchy");
    if local_script.exists() {
        return Ok(Some(fs::read_to_string(local_script)?));
    }

    let template_name = config.get_template().unwrap_or_else(|| "_dynamic".to_string());
    if let Some(path) = find_template_path(&template_name) {
        let embedded_path = format!("{}/app.ruchy", path);
        if let Some(file) = get_templates().get_file(&embedded_path) {
            return Ok(Some(file.contents_utf8().unwrap().to_string()));
        }
    }
    Ok(None)
}

use anyhow::Result;
use include_dir::{Dir, include_dir};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// Embed the templates directory relative to this file's location in the crate
pub static TEMPLATES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

pub fn get_templates() -> &'static Dir<'static> {
    &TEMPLATES_DIR
}

/// Finds the full path within the embedded directory for a given short name (e.g. "blink")
pub fn find_template_path(name: &str) -> Option<String> {
    if name == "_dynamic" {
        return Some("_dynamic".to_string());
    }

    // Search for the template folder in categories
    for entry in TEMPLATES_DIR.find("**/*").ok()? {
        if let Some(dir) = entry.as_dir()
            && dir.path().file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(dir.path().to_string_lossy().to_string());
            }
    }
    None
}

/// Helper to list examples for the CLI
pub fn list_examples_by_category() -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for entry in TEMPLATES_DIR.dirs() {
        let category = entry
            .path()
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if category.starts_with('_') {
            continue;
        } // skip _dynamic

        let mut examples = Vec::new();
        for sub in entry.dirs() {
            examples.push(
                sub.path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            );
        }
        examples.sort();
        if !examples.is_empty() {
            map.insert(category, examples);
        }
    }
    map
}

/// Processes a template directory: renders .tera files and copies others
pub fn process_template_directory(
    template_path: &str,
    project_name: &str,
    context: &tera::Context,
) -> Result<()> {
    let root = get_templates();
    let dir = root
        .get_dir(template_path)
        .ok_or_else(|| anyhow::anyhow!("Template dir not found: {}", template_path))?;

    for entry in dir.find("**/*")? {
        if let Some(file) = entry.as_file() {
            let path = file.path();
            // Get path relative to the specific template folder
            let relative_path = path.strip_prefix(template_path).unwrap_or(path);
            let dest_path = Path::new(project_name).join(relative_path);

            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if name.ends_with(".tera") {
                let content = file
                    .contents_utf8()
                    .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 in template"))?;
                // Render
                let rendered = tera::Tera::one_off(content, context, true)
                    .map_err(|e| anyhow::anyhow!("Template error in {}: {}", name, e))?;

                // Save without .tera extension
                let dest_clean = dest_path.with_extension("");
                fs::write(dest_clean, rendered)?;
            } else {
                // Copy raw file
                fs::write(&dest_path, file.contents())?;
            }
        }
    }
    Ok(())
}

/// Copies a file verbatim from the embedded directory to disk
pub fn copy_verbatim(file: &include_dir::File, root_path: &Path, target_base: &str) -> Result<()> {
    // Calculate path relative to the template root to preserve structure
    let relative_path = file.path().strip_prefix(root_path).unwrap_or(file.path());
    let dest = Path::new(target_base).join(relative_path);

    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(dest, file.contents())?;
    Ok(())
}

/// Helper to write generated config string to disk
pub fn write_template(path: &Path, content: &str) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(path, content)?;
    Ok(())
}

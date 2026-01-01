use include_dir::{Dir, include_dir};
use std::error::Error;
use tera::{Context, Tera};

pub static TEMPLATES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// Renders the main.rs file with the provided setup and forever code.
///
/// # Arguments
/// * `template_name` - Optional name of the template directory to use. Defaults to "default".
/// * `setup_code` - The code body for the setup section.
/// * `forever_code` - The code body for the loop section.
pub fn render_main(
    template_name: Option<&str>,
    setup_code: &str,
    forever_code: &str,
) -> Result<String, Box<dyn Error>> {
    let mut tera = Tera::default();

    let template = template_name.unwrap_or("default");
    let file_path = format!("{}/src/bin/main.rs", template);

    // Load the specific template file
    let template_content = TEMPLATES_DIR
        .get_file(&file_path)
        .ok_or(format!(
            "Template file '{}' not found in templates directory",
            file_path
        ))?
        .contents_utf8()
        .ok_or("Template is not valid UTF-8")?;

    tera.add_raw_template("main.rs", template_content)?;

    let mut context = Context::new();
    context.insert("setup_code", setup_code);
    context.insert("forever_code", forever_code);

    let rendered = tera.render("main.rs", &context)?;
    Ok(rendered)
}

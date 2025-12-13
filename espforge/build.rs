use std::{env, fs, io::Write, path::Path};

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generated_templates.rs");
    let mut file = fs::File::create(&dest_path).unwrap();

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let templates_dir_path = Path::new(&manifest_dir).join("templates");

    let mut module_declarations = Vec::new();
    let mut match_arms = Vec::new();

    if templates_dir_path.exists() {
        find_templates(
            &templates_dir_path,
            &mut module_declarations,
            &mut match_arms,
        );
    }

    // 3. Write the combined, fully dynamic code to the output file
    write!(
        &mut file,
        r#"
        use anyhow::Result;
        use std::collections::HashMap;
        use serde_yaml_ng::Value;

        // --- Generated Module Declarations ---
        {modules}

        // --- Generated Context Creation Function ---
        /// Create a type-safe Tera context from template name and properties
        pub fn create_context(
            name: &str, 
            example_properties: &HashMap<String, Value>
        ) -> Result<tera::Context> {{
            
            match name {{
                {match_arms}
                _ => {{
                    // Template has no configuration or its config.rs is missing
                }}
            }}
            Ok(tera::Context::new())
        }}
    "#,
        modules = module_declarations.join("\n"),
        match_arms = match_arms.join("\n")
    )
    .unwrap();

    println!("cargo:rerun-if-changed=templates");
}

// Recursively find directories containing config.rs
fn find_templates(dir: &Path, modules: &mut Vec<String>, arms: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // If this directory has a config.rs, it is a template
                if path.join("config.rs").exists() {
                    let template_name = path.file_name().unwrap().to_str().unwrap();
                    let config_path = path.join("config.rs");

                    // We use the directory name as the module name.
                    // This assumes template names are unique across categories.
                    modules.push(format!(
                        r#"#[path = r"{}"] pub mod {};"#,
                        config_path.display(),
                        template_name
                    ));

                    arms.push(format!(
                        r#""{template_name}" => {{
                            let value = serde_yaml_ng::to_value(example_properties)?;
                            let config: self::{template_name}::Config = 
                                serde_yaml_ng::from_value(value)?;
                            
                            return tera::Context::from_serialize(&config).map_err(anyhow::Error::from);
                        }},"#
                    ));
                }

                // Continue recursing to check subdirectories
                find_templates(&path, modules, arms);
            }
        }
    }
}

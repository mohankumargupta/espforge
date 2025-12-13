use crate::generate;
use anyhow::{Error, Result};
use std::fs;
use std::path::Path;

use crate::config::{EspforgeConfig, EspforgeConfiguration};
use crate::nibblers::{NibblerDispatcher, NibblerStatus};
use crate::resolver::{ContextResolver, ruchy_bridge};
use crate::template_utils::{find_template_path, get_templates, process_template_directory};

use serde::Serialize;

#[derive(Serialize)]
struct EspforgeContext<'a> {
    #[serde(flatten)]
    espforge: &'a EspforgeConfig,
    target: &'a str,
}

pub fn compile<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    let configuration_contents = fs::read_to_string(&path)?;
    let config: EspforgeConfiguration = serde_yaml_ng::from_str(&configuration_contents)?;

    println!("Running configuration checks...");
    let dispatcher = NibblerDispatcher::new();
    let results = dispatcher.process_config(&config);
    let mut validation_failed = false;

    for result in results {
        if !result.findings.is_empty() {
            println!("== {} ==", result.nibbler_name);
            for finding in result.findings {
                let prefix = match result.status {
                    NibblerStatus::Error => "❌",
                    NibblerStatus::Warning => "⚠️",
                    NibblerStatus::Ok => "✅",
                };
                println!("  {} {}", prefix, finding);
            }
        }

        if result.status == NibblerStatus::Error {
            validation_failed = true;
        }
    }

    if validation_failed {
        anyhow::bail!("Configuration validation failed due to errors above.");
    }

    let project_name = config.get_name();
    let esp32_platform = config.get_platform();

    println!("Generating project '{}'...", project_name);
    generate::generate(project_name, &esp32_platform)?;
    println!("Project generation complete.");

    let espforge_context = EspforgeContext {
        espforge: &config.espforge,
        target: config.espforge.platform.target(),
    };

    let mut context = if let Some(example_config) = &config.example {
        crate::templates::create_context(&example_config.name, &example_config.example_properties)?
    } else {
        tera::Context::new()
    };
    context.insert("espforge", &espforge_context);

    // Resolve the raw template name to a full path
    let template_name_raw = config
        .get_template()
        .unwrap_or_else(|| "_dynamic".to_string());

    let template_path = if template_name_raw == "_dynamic" {
        "_dynamic".to_string()
    } else {
        find_template_path(&template_name_raw)
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found", template_name_raw))?
    };

    let manifests = generate::load_manifests()?;
    let mut resolver = ContextResolver::new();
    let mut render_ctx = resolver.resolve(&config, &manifests)?;

    // Check for Ruchy script
    let config_dir = path.as_ref().parent().unwrap_or_else(|| Path::new("."));
    let local_ruchy_path = config_dir.join("app.ruchy");

    let ruchy_source_opt = if local_ruchy_path.exists() {
        println!("Found local Ruchy script: {}", local_ruchy_path.display());
        Some(fs::read_to_string(&local_ruchy_path)?)
    } else {
        let templates = get_templates();
        // Use full resolved path to find the embedded ruchy file
        let embedded_path = format!("{}/app.ruchy", template_path);
        if let Some(file) = templates.get_file(&embedded_path) {
            println!("Found embedded Ruchy script: {}", embedded_path);
            Some(
                file.contents_utf8()
                    .ok_or_else(|| anyhow::anyhow!("Embedded Ruchy file is not valid UTF-8"))?
                    .to_string(),
            )
        } else {
            None
        }
    };

    // Combine variables from YAML (legacy) and Ruchy (modern)
    let mut combined_variables = render_ctx.variables.clone();

    if let Some(raw_source) = ruchy_source_opt {
        let ruchy_out = ruchy_bridge::compile_ruchy_script(&raw_source)?;

        if !ruchy_out.setup.is_empty() {
            render_ctx.setup_code.push(ruchy_out.setup);
        }
        if !ruchy_out.loop_body.is_empty() {
            render_ctx.loop_code.push(ruchy_out.loop_body);
        }
        // Inject top-level Ruchy variables
        combined_variables.extend(ruchy_out.variables);
    }

    context.insert("includes", &render_ctx.includes);
    context.insert("initializations", &render_ctx.initializations);
    context.insert("variables", &combined_variables);
    context.insert("setup_code", &render_ctx.setup_code);
    context.insert("loop_code", &render_ctx.loop_code);

    // 1. Apply _dynamic template first
    process_template_directory("_dynamic", project_name, &context)?;

    // 2. Apply specific template
    if template_path != "_dynamic" {
        process_template_directory(&template_path, project_name, &context)?;
    }

    // 3. Local overrides

    // 3a. Handle local wokwi.toml.tera
    // This allows the downloaded example to have a template file that gets rendered
    // correctly based on the current platform/project name during compilation.
    let local_wokwi_tera = config_dir.join("wokwi.toml.tera");
    if local_wokwi_tera.exists() {
        println!("Applying local override: wokwi.toml.tera");
        let content = fs::read_to_string(&local_wokwi_tera)?;
        let rendered = tera::Tera::one_off(&content, &context, true)?;

        // Save as wokwi.toml in the target project directory
        let target_path = Path::new(project_name).join("wokwi.toml");
        fs::write(&target_path, rendered)?;
    }

    // 3b. Handle verbatim local file overrides (highest precedence)
    let overrides = ["diagram.json", "wokwi.toml", "chip.wasm", "chip.json"];
    for filename in overrides {
        let local_path = config_dir.join(filename);
        if local_path.exists() {
            println!("Applying local override: {}", filename);
            let target_path = Path::new(project_name).join(filename);
            fs::copy(&local_path, &target_path)?;
        }
    }

    Ok(())
}

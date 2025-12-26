use crate::config::{EspforgeConfig, EspforgeConfiguration};
use crate::resolver::{ruchy_bridge, ContextResolver};
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct EspforgeMeta<'a> {
    #[serde(flatten)]
    espforge: &'a EspforgeConfig,
    target: &'a str,
}

pub fn prepare_tera_context(config: &EspforgeConfiguration) -> Result<tera::Context> {
    let mut context = match &config.example {
        Some(ex) => crate::templates::create_context(&ex.name, &ex.example_properties)?,
        None => tera::Context::new(),
    };

    context.insert("espforge", &EspforgeMeta {
        espforge: &config.espforge,
        target: config.espforge.platform.target(),
    });
    
    Ok(context)
}

pub fn resolve_application_logic(
    config: &EspforgeConfiguration,
    config_dir: &Path,
    context: &mut tera::Context,
) -> Result<()> {
    let manifests = crate::generate::load_manifests()?;
    let mut resolver = ContextResolver::new();
    let mut render_ctx = resolver.resolve(config, &manifests)?;

    if let Some(source) = super::scripting::load_ruchy_source(config, config_dir)? {
        let ruchy = ruchy_bridge::compile_ruchy_script(&source, config.espforge.enable_async)?;
        render_ctx.setup_code.push(ruchy.setup);
        render_ctx.loop_code.push(ruchy.loop_body);
        render_ctx.task_definitions.extend(ruchy.task_definitions);
        render_ctx.task_spawns.extend(ruchy.task_spawns);
        render_ctx.variables.extend(ruchy.variables);
    }

    context.insert("includes", &render_ctx.includes);
    context.insert("initializations", &render_ctx.initializations);
    context.insert("variables", &render_ctx.variables);
    context.insert("setup_code", &render_ctx.setup_code);
    context.insert("loop_code", &render_ctx.loop_code);
    context.insert("task_definitions", &render_ctx.task_definitions);
    context.insert("task_spawns", &render_ctx.task_spawns);

    Ok(())
}


use anyhow::{Error, Result};
use std::path::{Path, PathBuf};

mod validation;
mod context;
mod scripting;
mod template;
mod postprocess;

pub fn compile<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    let config_path = path.as_ref();
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    
    let config = validation::load_and_validate(config_path)?;

    let project_name = config.get_name();
    crate::generate::generate(
        project_name,
        &config.get_platform(),
        config.espforge.enable_async,
    )?;

    let project_path = PathBuf::from(project_name);

    let mut tera_context = context::prepare_tera_context(&config)?;
    context::resolve_application_logic(&config, config_dir, &mut tera_context)?;    
    template::apply_templates(&config, &project_path, &tera_context)?;
    postprocess::refine_project_files(&project_path, config_dir, &tera_context)?;

    Ok(())
}

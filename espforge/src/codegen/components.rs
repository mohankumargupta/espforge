use anyhow::Result;
use espforge_common::ProjectModel;

pub fn generate_components_source(model: &ProjectModel) -> Result<String> {
    espforge_codegen::generate_components_source(model)
}


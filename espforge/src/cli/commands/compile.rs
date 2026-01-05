use crate::compile;
use anyhow::Result;
use std::path::Path;

pub fn execute(file: &Path) -> Result<()> {
    compile::compile_project(file)
}

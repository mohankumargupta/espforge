use std::path::Path;
use anyhow::Result;
use crate::compile;

pub fn execute(file: &Path) -> Result<()> {
    compile::compile_project(file)
}

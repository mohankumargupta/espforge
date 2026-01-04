use std::path::Path;
use anyhow::Result;
use crate::compile;

pub fn execute(file: &Path) -> Result<()> {
    // Delegate entirely to the compile module orchestrator
    compile::compile_project(file)
}

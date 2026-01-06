use include_dir::{Dir, include_dir};

pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");
pub static EXTRA_DEPENDENCIES: &'static str = include_str!("../dependencies.toml");


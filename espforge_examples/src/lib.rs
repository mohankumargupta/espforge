use include_dir::{Dir, include_dir};

// Embeds the 'examples' directory found at the root of this crate
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");

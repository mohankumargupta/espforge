pub mod cli;
pub mod compile;
pub mod parse;
pub mod examples;


use include_dir::{Dir, include_dir};
pub static PLATFORM_SRC: Dir = include_dir!("$CARGO_MANIFEST_DIR/../espforge_platform");

pub mod cli;
pub mod codegen;
pub mod compile;
pub mod parse;
pub mod resolve;
pub mod validate;

use include_dir::{Dir, include_dir};
pub static PLATFORM_SRC: Dir = include_dir!("$CARGO_MANIFEST_DIR/../espforge_platform");

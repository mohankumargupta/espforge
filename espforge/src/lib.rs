pub mod cli;
pub mod compile;
pub mod examples;
pub mod parse;

use include_dir::{Dir, include_dir};
pub static PLATFORM_SRC: Dir = include_dir!("$CARGO_MANIFEST_DIR/../espforge_platform");
pub static DEVICES_SRC: Dir = include_dir!("$CARGO_MANIFEST_DIR/../espforge_devices");

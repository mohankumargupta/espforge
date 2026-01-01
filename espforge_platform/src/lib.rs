// pub mod logger;

// pub struct Context {
//     pub logger: logger::Logger,
// }

// impl Context {
//     pub fn new() -> Self {
//         Self {
//             logger: logger::Logger,
//         }
//     }
// }

use include_dir::{Dir, include_dir};

pub static PLATFORM_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets");

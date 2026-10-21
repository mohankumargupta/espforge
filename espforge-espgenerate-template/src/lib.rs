use include_dir::{Dir, include_dir};

/// Version of this template crate. Recorded in `.espforge/manifest.json` so a
/// generated project says which template produced it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The project template tree, embedded at compile time from `src/template`.
/// Rendered by `espforge-emit` (see its `render` module).
pub static TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/template");

/// The option schema (KDL v2), embedded at compile time.
/// Loaded by `espforge-options`.
pub const OPTIONS_KDL: &str = include_str!("options.kdl");

/// The element catalog (KDL v2) behind `espforge espf`, embedded at compile
/// time. Loaded by `espforge-options::catalog`.
pub const ELEMENTS_KDL: &str = include_str!("elements.kdl");

/// The prose of `espforge espf overview`, embedded at compile time.
pub const OVERVIEW_TXT: &str = include_str!("overview.txt");

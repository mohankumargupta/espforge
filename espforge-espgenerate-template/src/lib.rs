/// Version of this crate. Recorded in `.espforge/manifest.json` so a
/// generated project says which template produced it.
///
/// The project files themselves are no longer template files: `espforge-emit`
/// generates every one of them from typed backends (Phase 3). This crate now
/// carries the schema and the prose those backends and the CLI are built from.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The schema (KDL v2): the single source of truth for the elements ESPF
/// accepts and for the build options. Embedded at compile time; the element
/// half is loaded by `espforge-options::catalog` for `espforge espf`, the
/// option half by `espforge-options::schema`, and the element half is also read
/// at build time by `espforge-bindings` to generate its types.
pub const SCHEMA_KDL: &str = include_str!("espf.schema.kdl");

/// The prose of `espforge espf overview`, embedded at compile time.
pub const OVERVIEW_TXT: &str = include_str!("overview.txt");

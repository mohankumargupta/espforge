//! The schema's option half, option resolution, and the `# espforge` section
//! reader. Both halves of the schema are the same file (`espf.schema.kdl`).
//!
//! * [`catalog`] - the element half behind `espforge espf`: types, loader and
//!   path resolution
//! * [`schema`]  - the option half: types and the KDL v2 loader
//! * [`resolve`] - 2.2-2.4: defaults, derived flags, requires, chip
//!   compatibility, selection groups, console-vs-chip
//! * [`section`] - 2.5: `core_from_section`, replacing `parse_meta`

pub mod catalog;
mod generated_options;
pub mod chips;
pub mod resolve;
pub mod schema;
pub mod section;

pub use resolve::{
    Answer, Answers, DerivedInputs, Origin, Resolved, ResolvedOptions, Why, resolve, resolve_with,
};
pub use schema::{Compat, Kind, OptValue, OptionDef, Requires, Schema, embedded_schema, load};
pub use section::core_from_section;

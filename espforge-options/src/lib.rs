//! Option schema (`options.kdl`), option resolution, and the `# espforge`
//! section reader.
//!
//! * [`schema`]  - 2.1: schema types and the KDL v2 loader
//! * [`resolve`] - 2.2-2.4: defaults, derived flags, requires, chip
//!   compatibility, selection groups, console-vs-chip
//! * [`section`] - 2.5: `core_from_section`, replacing `parse_meta`

pub mod chips;
pub mod resolve;
pub mod schema;
pub mod section;

pub use resolve::{
    Answer, Answers, DerivedInputs, Origin, Resolved, ResolvedOptions, Why, resolve, resolve_with,
};
pub use schema::{Compat, Kind, OptValue, OptionDef, Requires, Schema, embedded_schema, load};
pub use section::core_from_section;

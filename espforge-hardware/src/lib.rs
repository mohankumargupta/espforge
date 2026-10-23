//! Hardware lowering: validated spec -> [`Hardware`](espforge_model::hardware::Hardware).
//!
//! Created in step 1a (see `improve.md` item 1). The spec structs already live
//! in [`espforge_model::hardware`]; the per-bus lowering (`wifi`, `i2c`,
//! `spi`, `uart`, `gpio`/`led`/`button`) and the pin/peripheral conflict
//! checks move here from `espforge/src/commands/pipeline.rs` in steps
//! 1d–1e. Until then this crate only re-exports the model types so the
//! public function shape (`&Checked -> Result<Hardware, String>`) is fixed.
//!
//! Planned layout (from item 1):
//!
//! ```text
//! mod gpio, i2c, spi, uart, wifi, conflicts  # one bus each + checks
//! pub fn lower(...) -> Result<Hardware, String>
//! pub fn buildable(...)                       # moved from pipeline.rs:1048
//! ```

// Re-export so lowering modules and callers share one import path.
pub use espforge_model::hardware::*;

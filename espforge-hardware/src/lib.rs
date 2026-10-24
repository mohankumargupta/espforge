//! Hardware lowering: validated sections -> typed [`Board`](board::Board).
//!
//! `board::build` resolves the `$` graph, builds every peripheral in source
//! order, then every component, claiming each pin as it goes. The emitter
//! still reads the seven-`Vec` [`Hardware`] projection until Phase 3 replaces
//! its templates; [`espforge_model::board::Board::hardware`] is that bridge.
//!
//! Per-bus attribute parsing lives in [`peripherals`], services in
//! [`services`], and the `# devices` guard in [`devices`]. The pin/peripheral
//! ownership rules are the claims in `espforge-model`, so there is no second
//! conflict pass.

pub mod board;
pub mod devices;
pub mod peripherals;
pub mod services;
pub mod util;

// Re-export so lowering modules and callers share one import path.
pub use espforge_model::hardware::*;

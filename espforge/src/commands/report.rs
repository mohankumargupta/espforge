//! How a pipeline failure looks on a terminal: snippet blocks for located
//! problems, the historical one-line `❌` for a bad flag.
//!
//! The pipeline returns typed failures (`Invalid`); this module is the only
//! place that knows they are printed to stderr and how they are phrased.

use espforge_core::pipeline::Invalid;
use espforge_diagnostics::{Diagnostic, to_human};

/// Print each diagnostic as a snippet block, then a one-line summary.
pub fn diagnostics(name: &str, src: &str, diagnostics: &[Diagnostic]) {
    eprintln!("{}", to_human(name, src, diagnostics));
    eprintln!(
        "❌ Found {} problem(s) in '{}'; fix them and re-run.",
        diagnostics.len(),
        name
    );
}

/// Render a pipeline failure: snippets when the problem is located, the
/// historical `❌ <message>` line when it came from a flag.
pub fn invalid(failure: &Invalid, name: &str, src: &str) {
    match failure.message() {
        Some(message) => eprintln!("❌ {message}"),
        None => diagnostics(name, src, &failure.diagnostics()),
    }
}

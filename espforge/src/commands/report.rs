//! How a failed generation looks on a terminal: snippet blocks for located
//! problems, the historical one-line `❌` for a bad flag.
//!
//! The library returns typed failures; this module is the only place that
//! knows they are printed to stderr and how they are phrased.

use espforge_diagnostics::{Diagnostic, to_human};

/// Print the `note:` and warning lines a command produced.
pub fn notes(notes: &[String]) {
    for note in notes {
        eprintln!("{note}");
    }
}

/// Print each diagnostic as a snippet block, then a one-line summary.
pub fn diagnostics(name: &str, src: &str, diagnostics: &[Diagnostic]) {
    eprintln!("{}", to_human(name, src, diagnostics));
    eprintln!(
        "❌ Found {} problem(s) in '{}'; fix them and re-run.",
        diagnostics.len(),
        name
    );
}

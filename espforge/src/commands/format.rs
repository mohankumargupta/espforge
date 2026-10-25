//! `--format`: whether a command talks to a person or to a program.
//!
//! The flag and the JSON writer live here rather than in `check.rs`, so a
//! command that grows a machine mode reuses the same spelling, the same
//! document printing and the same trailing newline. A command only offers the
//! flag when a machine consumer exists for it (`check` today); the default is
//! always [`Format::Human`], so adding the flag never changes existing output.

use clap::ValueEnum;
use serde_json::Value;

#[derive(ValueEnum, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    /// Snippets on stderr, `✓ ... is valid` on stdout.
    #[default]
    Human,
    /// One JSON document on stdout: diagnostics and the board.
    Json,
}

/// Write one JSON document to stdout, pretty-printed, one trailing newline.
pub fn emit(document: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(document).expect("a serde_json::Value is serializable")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_format_is_human() {
        assert_eq!(Format::default(), Format::Human);
    }

    #[test]
    fn the_cli_spellings_are_human_and_json() {
        assert_eq!(
            Format::value_variants()
                .iter()
                .map(|f| f.to_possible_value().unwrap().get_name().to_string())
                .collect::<Vec<_>>(),
            ["human", "json"]
        );
    }
}

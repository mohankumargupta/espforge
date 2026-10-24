//! Semantic comparison for the dual-run gate (Phase 3).
//!
//! Byte equality is too strict once `prettyplease` owns Rust layout, and too
//! weak for TOML/JSON where key order and whitespace are not meaningful. These
//! helpers compare what the file *means*:
//!
//! * Rust: parse both sides with `syn`, print both with `prettyplease`, compare
//! * TOML: parse both sides into `toml::Value`
//! * JSON: parse both sides into `serde_json::Value`
//!
//! They are not a diff tool. On mismatch they return the first differing line
//! of the normalized form, which is enough for a test failure to be actionable.

use anyhow::{Context, Result, bail};

/// Compare two Rust files modulo formatting.
pub fn same_rust(left: &str, right: &str) -> Result<()> {
    let left = syn::parse_file(left).context("left side is not parseable Rust")?;
    let right = syn::parse_file(right).context("right side is not parseable Rust")?;
    let left = prettyplease::unparse(&left);
    let right = prettyplease::unparse(&right);
    if left == right {
        return Ok(());
    }
    bail!("Rust differs:\n{}", first_difference(&left, &right))
}

/// Compare two TOML files by value.
pub fn same_toml(left: &str, right: &str) -> Result<()> {
    let left: toml::Value = toml::from_str(left).context("left side is not valid TOML")?;
    let right: toml::Value = toml::from_str(right).context("right side is not valid TOML")?;
    if left == right {
        return Ok(());
    }
    bail!("TOML differs:\nleft:  {left:?}\nright: {right:?}")
}

/// Compare two JSON files by value.
pub fn same_json(left: &str, right: &str) -> Result<()> {
    let left: serde_json::Value =
        serde_json::from_str(left).context("left side is not valid JSON")?;
    let right: serde_json::Value =
        serde_json::from_str(right).context("right side is not valid JSON")?;
    if left == right {
        return Ok(());
    }
    bail!("JSON differs:\nleft:  {left}\nright: {right}")
}

/// The first line that differs in the normalized text, or a length note.
fn first_difference(left: &str, right: &str) -> String {
    for (index, (a, b)) in left.lines().zip(right.lines()).enumerate() {
        if a != b {
            return format!("line {}:\n  left:  {a}\n  right: {b}", index + 1);
        }
    }
    format!(
        "line count differs: left {}, right {}",
        left.lines().count(),
        right.lines().count()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_layout_differences_are_equal() {
        let left = "fn main(){let x=1;}\n";
        let right = "fn main() {\n    let x = 1;\n}\n";
        same_rust(left, right).unwrap();
    }

    #[test]
    fn a_changed_rust_token_is_rejected() {
        let left = "fn main() { let x = 1; }\n";
        let right = "fn main() { let x = 2; }\n";
        let err = same_rust(left, right).unwrap_err().to_string();
        assert!(err.contains("Rust differs"), "{err}");
        assert!(err.contains("let x = 2"), "{err}");
    }

    #[test]
    fn rust_that_does_not_parse_is_an_error_not_a_false_match() {
        assert!(same_rust("fn main( {", "fn main() {}").is_err());
        assert!(same_rust("fn main() {}", "fn main( {").is_err());
    }

    #[test]
    fn toml_layout_and_key_order_are_equal() {
        let left = "a = 1\nb = \"x\"\n";
        let right = "b = \"x\"\na = 1\n";
        same_toml(left, right).unwrap();
    }

    #[test]
    fn a_changed_toml_value_is_rejected() {
        assert!(same_toml("a = 1\n", "a = 2\n").is_err());
        assert!(same_toml("a = 1\n", "not toml").is_err());
    }

    #[test]
    fn json_value_equality_ignores_layout_and_key_order() {
        let left = r#"{"a": 1, "b": [true, null]}"#;
        let right = "{\n  \"b\": [\n    true,\n    null\n  ],\n  \"a\": 1\n}\n";
        same_json(left, right).unwrap();
    }

    #[test]
    fn a_changed_json_value_is_rejected() {
        assert!(same_json(r#"{"a": 1}"#, r#"{"a": 2}"#).is_err());
        assert!(same_json(r#"{"a": 1}"#, "not json").is_err());
    }
}

//! Semantic comparison for the dual-run gate (Phase 3).
//!
//! Byte equality is too strict once `prettyplease` owns Rust layout, and too
//! weak for TOML/JSON where key order and whitespace are not meaningful. These
//! helpers compare what the file *means*:
//!
//! * Rust: parse both sides with `syn`, compare the resulting token trees
//! * TOML: parse both sides into `toml::Value`
//! * JSON: parse both sides into `serde_json::Value`
//!
//! They are not a diff tool. On mismatch they point at the first difference (a
//! token with a few neighbours, or the parsed values), which is enough for a
//! test failure to be actionable.

use anyhow::{Context, Result, bail};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;

/// Compare two Rust files modulo formatting.
///
/// Tokens, not text: `prettyplease` leaves macro bodies as it found them, so
/// two identical `macro_rules!` bodies can print differently depending on the
/// whitespace in the source they came from.
pub fn same_rust(left: &str, right: &str) -> Result<()> {
    let left_tokens = normal_form(left).context("left side is not parseable Rust")?;
    let right_tokens = normal_form(right).context("right side is not parseable Rust")?;
    if left_tokens == right_tokens {
        return Ok(());
    }
    let at = first_difference(&left_tokens, &right_tokens);
    bail!(
        "Rust differs at token {at}:\n  left:  {}\n  right: {}",
        describe(&left_tokens, at),
        describe(&right_tokens, at)
    )
}

/// Parse, prettyprint, parse again, and tokenize. The round trip through
/// `prettyplease` canonicalizes the differences it introduces itself (`()` to
/// `{}`, a nested `match` arm to a block), while tokenizing afterwards ignores
/// the layout it chooses — including inside macro bodies, which it leaves as
/// the source wrote them.
fn normal_form(text: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(text)?;
    let printed = prettyplease::unparse(&file);
    let file = syn::parse_file(&printed)?;
    Ok(words(&file.to_token_stream()))
}

/// A canonical token sequence: groups contribute their delimiters as words of
/// their own and every token becomes one word, so two programs compare equal
/// iff they mean the same thing regardless of layout. Punctuation is split into
/// characters on purpose: `&$x` and `& $x` are the same program, and proc-macro2
/// reports `&` as *joint* only because the source had no space after it.
fn words(tokens: &TokenStream) -> Vec<String> {
    fn walk(tokens: &TokenStream, out: &mut Vec<String>) {
        for token in tokens.clone() {
            match token {
                TokenTree::Group(group) => {
                    let (open, close) = match group.delimiter() {
                        Delimiter::Parenthesis => ("(", ")"),
                        Delimiter::Brace => ("{", "}"),
                        Delimiter::Bracket => ("[", "]"),
                        Delimiter::None => ("", ""),
                    };
                    if !open.is_empty() {
                        out.push(open.to_string());
                    }
                    walk(&group.stream(), out);
                    if !close.is_empty() {
                        out.push(close.to_string());
                    }
                }
                TokenTree::Ident(ident) => out.push(ident.to_string()),
                TokenTree::Punct(punct) => out.push(punct.as_char().to_string()),
                TokenTree::Literal(literal) => out.push(literal_key(&literal)),
            }
        }
    }
    let mut out = Vec::new();
    walk(tokens, &mut out);
    out
}

/// Literals compare by value, not by spelling: `"a\n  b"` written with a line
/// continuation is the same string as one written on a single line, and `0x10`
/// is the same number as `16`.
fn literal_key(literal: &proc_macro2::Literal) -> String {
    use syn::Lit;
    match Lit::new(literal.clone()) {
        Lit::Str(lit) => format!("{:?}", lit.value()),
        Lit::ByteStr(lit) => format!("{:?}", lit.value()),
        Lit::CStr(lit) => format!("{:?}", lit.value()),
        Lit::Byte(lit) => format!("{}", lit.value()),
        Lit::Char(lit) => format!("{:?}", lit.value()),
        Lit::Int(lit) => format!("{}{}", lit.base10_digits(), lit.suffix()),
        Lit::Float(lit) => format!("{}{}", lit.base10_digits(), lit.suffix()),
        Lit::Bool(lit) => format!("{}", lit.value),
        _ => literal.to_string(),
    }
}

/// The index of the first differing word; the length of the shorter side when
/// one is a prefix of the other.
fn first_difference(left: &[String], right: &[String]) -> usize {
    left.iter()
        .zip(right)
        .position(|(a, b)| a != b)
        .unwrap_or(left.len().min(right.len()))
}

/// A few words of context around `index`, with a marker on the difference.
fn describe(words: &[String], index: usize) -> String {
    let start = index.saturating_sub(4);
    let end = (index + 5).min(words.len());
    let mut out = String::new();
    for (i, word) in words.iter().enumerate().take(end).skip(start) {
        if i != start {
            out.push(' ');
        }
        if i == index {
            out.push_str(&format!(">>>{word}<<<"));
        } else {
            out.push_str(word);
        }
    }
    out
}

/// Every comment of the first file must appear, in order, in the second.
///
/// `syn` drops comments, so [`same_rust`] cannot see them. The generated files
/// carry real content in `//` lines (the header, the `SAFETY` notes), so the
/// dual run checks that the backend lost none of them. The check is one-way
/// because the backend also adds comments the template did not have: the
/// "GENERATED BY ESPFORGE" header on all four files, and the wiring notes in
/// `main.rs`. Indentation and blank lines are ignored; `///` lines are
/// attributes and already part of the token comparison.
pub fn same_comments(reference: &str, generated: &str) -> Result<()> {
    fn comments(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim)
            .filter(|line| {
                line.starts_with("//")
                    && !line.starts_with("///")
                    && !line.starts_with("// GENERATED BY ESPFORGE")
                    && *line != "// Source of truth is the project .espf file."
            })
            .map(str::to_string)
            .collect()
    }
    let reference = comments(reference);
    let generated = comments(generated);
    let mut missing = reference.iter();
    for line in &generated {
        if missing.clone().next() == Some(line) {
            missing.next();
        }
    }
    if let Some(line) = missing.next() {
        bail!(
            "the generated file drops a comment:\n  missing: {line}\n  generated: {}",
            generated.join(" | ")
        );
    }
    Ok(())
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
        assert!(
            err.contains(">>>2<<<"),
            "the message points at the token: {err}"
        );
    }

    #[test]
    fn macro_bodies_are_compared_as_tokens_not_layout() {
        let left = "macro_rules! m { ($x:expr) => { &$x.components.$y }; }\n";
        let right = "macro_rules! m { ($x:expr) => { & $x .components. $y }; }\n";
        same_rust(left, right).unwrap();
        // but a changed token inside a macro body still fails
        let other = "macro_rules! m { ($x:expr) => { &$x.devices.$y }; }\n";
        assert!(same_rust(left, other).is_err());
    }

    #[test]
    fn comments_must_survive_but_may_be_added() {
        let reference = "// keep me\nfn main() {}\n";
        same_comments(reference, "  // keep me\n// added later\nfn main() {}\n").unwrap();
        let dropped = same_comments(reference, "fn main() {}\n").unwrap_err();
        assert!(dropped.to_string().contains("keep me"), "{dropped}");
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

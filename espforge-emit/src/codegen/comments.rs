//! Comment injection for generated Rust.
//!
//! `quote!`/`proc-macro2` has no comment tokens, and `prettyplease` does not
//! invent them. The generated project uses comments for real content: the
//! file header and the `SAFETY` justification on each unsafe `clone_unchecked`
//! call. This pass inserts those lines after printing, at anchors the backend
//! knows it emitted. Every anchor must match exactly once; a miss is a bug in
//! the backend, not something to ignore.

use anyhow::{Result, bail};

/// One comment block (without `// `) to place immediately before `anchor`.
pub struct Comment {
    pub anchor: String,
    pub lines: Vec<String>,
}

impl Comment {
    pub fn new(anchor: impl Into<String>, line: impl Into<String>) -> Self {
        Self {
            anchor: anchor.into(),
            lines: vec![line.into()],
        }
    }

    pub fn block(anchor: impl Into<String>, lines: &[&str]) -> Self {
        Self {
            anchor: anchor.into(),
            lines: lines.iter().map(|line| (*line).to_string()).collect(),
        }
    }
}

/// Put a comment block at the very top of a generated file.
pub fn prepend(text: String, lines: &[&str]) -> String {
    let mut out = lines
        .iter()
        .map(|line| format!("// {line}\n"))
        .collect::<String>();
    out.push_str(&text);
    out
}

/// Insert every comment before the unique output line containing its anchor.
///
/// Indentation is copied from the anchor line so the result is still
/// `prettyplease`-shaped. The caller re-parses the result; this function only
/// guarantees placement.
pub fn apply(mut text: String, comments: &[Comment]) -> Result<String> {
    for comment in comments {
        let matches: Vec<usize> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(&comment.anchor))
            .map(|(index, _)| index)
            .collect();
        if matches.len() != 1 {
            bail!(
                "comment anchor `{}` matched {} lines, expected exactly one",
                comment.anchor,
                matches.len()
            );
        }
        let index = matches[0];
        let trailing_newline = text.ends_with('\n');
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let indent: String = lines[index]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        for (offset, line) in comment.lines.iter().enumerate() {
            lines.insert(index + offset, format!("{indent}// {line}"));
        }
        text = lines.join("\n");
        if trailing_newline {
            text.push('\n');
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_before_the_unique_anchor_with_its_indent() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let out = apply(text.to_string(), &[Comment::new("let x", "why")]).unwrap();
        assert_eq!(out, "fn main() {\n    // why\n    let x = 1;\n}\n");
    }

    #[test]
    fn a_block_keeps_its_line_order() {
        let text = "fn main() {\n    call();\n}\n";
        let out = apply(
            text.to_string(),
            &[Comment::block("call()", &["one", "two"])],
        )
        .unwrap();
        assert_eq!(out, "fn main() {\n    // one\n    // two\n    call();\n}\n");
    }

    #[test]
    fn a_missing_or_ambiguous_anchor_is_an_error() {
        assert!(apply("fn main() {}\n".into(), &[Comment::new("let x", "why")]).is_err());
        assert!(apply("let x;\nlet x;\n".into(), &[Comment::new("let x", "why")]).is_err());
    }
}

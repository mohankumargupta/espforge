//! Span-aware ESPF diagnostics and the line helpers behind them.
//!
//! Every problem the parser and the value assembler can report is a
//! [`Diagnostic`] built here with a code from `espforge_diagnostics::codes`.
//! Rendering lives in `espforge_diagnostics::to_human`.

use espforge_diagnostics::{Diagnostic, codes};
use std::ops::Range;

/// The underlying cause of a parser-junk line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunkKind {
    /// A line inside a `# section` body that does not look like a
    /// `<tag attr="v" />` element (or is one with trailing garbage).
    NodeLine,
    /// A line that was expected to start a `# section` but did not.
    SectionHeader,
}

/// A byte offset (and line number) into the ESPF source that the parser
/// could not interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JunkSpan {
    pub offset: usize,
    pub line: usize,
    pub kind: JunkKind,
}

/// A diagnostic pointing at a parsed node's tag (`<tagname`).
pub fn node_span(node: &crate::context::ParsedNode) -> Range<usize> {
    let start = node.offset;
    start..start + 1 + node.tag_name.len()
}

/// A diagnostic pointing at an attribute's value (`key="<value>"`).
pub fn attr_span(attr: &crate::context::TagAttribute) -> Range<usize> {
    let start = attr.val_offset;
    start..start + attr.val.len()
}

/// A diagnostic pointing at an attribute's key (`<key>="value"`).
pub fn key_span(attr: &crate::context::TagAttribute) -> Range<usize> {
    let start = attr.key_offset;
    start..start + attr.key.len()
}

/// Line-start byte offsets of every line in `src` (0-indexed line numbers).
fn line_start_offsets(src: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            src.char_indices()
                .filter_map(|(i, c)| (c == '\n').then_some(i + 1)),
        )
        .collect()
}

/// Line number (1-based) of a byte offset in `src`.
///
/// `line_start_offsets` prepends byte offset 0, so simply counting entries
/// that start at or before `offset` gives the 1-based line number for free.
pub fn line_number_for_offset(src: &str, offset: usize) -> usize {
    line_start_offsets(src).partition_point(|&start| start <= offset)
}

/// Turn parser-junk spans into one [`Diagnostic`] per junk line, each with a
/// cause-specific message and label.
pub fn junk_diagnostics(junk: &[JunkSpan]) -> Vec<Diagnostic> {
    junk.iter()
        .map(|j| {
            let (code, message, label) = match j.kind {
                JunkKind::NodeLine => (
                    codes::SYNTAX_NODE_LINE,
                    "malformed node line",
                    "expected a self-closing tag like <tag attr=\"v\" /> that fills a whole line (trailing `// comment` is allowed)",
                ),
                JunkKind::SectionHeader => (
                    codes::SYNTAX_SECTION_HEADER,
                    "invalid section header",
                    "expected `# name` (one word after `#`)",
                ),
            };
            Diagnostic::error(code, j.offset..j.offset, message, label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_diagnostics::to_human;

    #[test]
    fn junk_line_renders_snippet() {
        let src = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n";
        let junk = vec![JunkSpan {
            offset: 14,
            line: 2,
            kind: JunkKind::NodeLine,
        }];
        let diagnostics = junk_diagnostics(&junk);
        let out = to_human("bad.espf.txt", src, &diagnostics);
        eprintln!("{out}");
        assert!(out.contains("malformed node line"));
        assert!(out.contains("bad.espf.txt:2"));
        assert_eq!(diagnostics[0].code, codes::SYNTAX_NODE_LINE);
    }

    #[test]
    fn section_header_junk_gets_its_own_title_and_code() {
        let src = "hello world\n# peripherals\n";
        let junk = vec![JunkSpan {
            offset: 0,
            line: 1,
            kind: JunkKind::SectionHeader,
        }];
        let diagnostics = junk_diagnostics(&junk);
        let out = to_human("bad.espf.txt", src, &diagnostics);
        assert!(out.contains("invalid section header"));
        assert!(out.contains("bad.espf.txt:1"));
        assert_eq!(diagnostics[0].code, codes::SYNTAX_SECTION_HEADER);
    }

    #[test]
    fn line_number_matches_offset() {
        let src = "# a\n# b\n# c\n";
        assert_eq!(line_number_for_offset(src, 0), 1);
        assert_eq!(line_number_for_offset(src, 4), 2);
        assert_eq!(line_number_for_offset(src, 8), 3);
        // The terminator of line 3 still counts as line 3...
        assert_eq!(line_number_for_offset(src, 11), 3);
        // ...but EOF after it is its own (empty) line 4.
        assert_eq!(line_number_for_offset(src, src.len()), 4);
    }
}

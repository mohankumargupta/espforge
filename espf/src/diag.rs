//! Span-aware ESPF errors, and the migration adapter for rendering them.
//!
//! Every checkable stage of the pipeline (parser, meta validation, `$`
//! dependency resolution) reports problems as one [`EspfError`] carrying a
//! byte-offset span into the original source plus a title and label. Rendering
//! moved to `espforge_diagnostics::to_human` in Phase 2 increment 1; this
//! module keeps the constructors, the line helpers and [`render_errors`] as a
//! thin adapter, so existing call sites migrate in increment 5:
//!
//! ```text
//! error: unknown runtime "fome"
//!   --> hello.espf:3:40
//!    |
//!  3 |   <runtime type="fome" />
//!    |                          ^^ unknown runtime "fome" (expected "blocking" or "embassy")
//! ```
//!
//! Colors are only emitted when stderr is a terminal; piped or captured
//! output stays plain so it can be grepped or redirected safely. That decision
//! now lives in the renderer.

use espforge_diagnostics::Diagnostic;
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

/// A single located diagnostic: where it points, why, and a summary title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspfError {
    /// Short summary shown on the `error:` line.
    pub title: String,
    /// Explanation rendered at the underline.
    pub label: String,
    /// Byte span into the original source the label points at. A zero-length
    /// span underlines one character.
    pub span: Range<usize>,
}

impl EspfError {
    /// An error with an explicit byte span.
    pub fn at_span(span: Range<usize>, title: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            label: label.into(),
            span,
        }
    }

    /// An error pointing at a parsed node's tag (`<tagname`).
    pub fn at_node(
        node: &crate::context::ParsedNode,
        title: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        let start = node.offset;
        Self {
            title: title.into(),
            label: label.into(),
            span: start..start + 1 + node.tag_name.len(),
        }
    }

    /// An error pointing at an attribute's value (`key="<value>"`).
    pub fn at_attr(
        attr: &crate::context::TagAttribute,
        title: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        let start = attr.val_offset;
        Self {
            title: title.into(),
            label: label.into(),
            span: start..start + attr.val.len(),
        }
    }

    /// An error pointing at an attribute's key (`<key>="value"`).
    pub fn at_key(
        attr: &crate::context::TagAttribute,
        title: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        let start = attr.key_offset;
        Self {
            title: title.into(),
            label: label.into(),
            span: start..start + attr.key.len(),
        }
    }

    /// This error in the one diagnostic model, for rendering and JSON.
    ///
    /// The stable code comes from the message table in
    /// [`espforge_diagnostics::codes`]; the human renderer does not print it.
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::error(
            espforge_diagnostics::for_message(&self.title),
            self.span.clone(),
            self.title.clone(),
            self.label.clone(),
        )
    }
}

/// Byte span of the offending line's node, for errors that have no better
/// anchor (e.g. a missing attribute on a node).
pub fn node_span(node: &crate::context::ParsedNode) -> Range<usize> {
    let start = node.offset;
    start..start + 1 + node.tag_name.len()
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

/// Turn parser-junk spans into one [`EspfError`] per junk line, each with a
/// cause-specific title and label.
pub fn junk_errors(junk: &[JunkSpan]) -> Vec<EspfError> {
    junk.iter()
        .map(|j| {
            let (title, label) = match j.kind {
                JunkKind::NodeLine => (
                    "malformed node line",
                    "expected a self-closing tag like <tag attr=\"v\" /> that fills a whole line (trailing `// comment` is allowed)",
                ),
                JunkKind::SectionHeader => (
                    "invalid section header",
                    "expected `# name` (one word after `#`)",
                ),
            };
            EspfError {
                title: title.into(),
                label: label.into(),
                span: j.offset..j.offset,
            }
        })
        .collect()
}

/// Render each error as its own snippet group through the one diagnostics
/// renderer. `EspfError` is the migration shape: Phase 2 increment 5 replaces
/// it with `Diagnostic` constructors at each call site.
pub fn render_errors(file_path: &str, src: &str, errors: &[EspfError]) -> String {
    let diagnostics: Vec<Diagnostic> = errors.iter().map(EspfError::to_diagnostic).collect();
    espforge_diagnostics::to_human(file_path, src, &diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_line_renders_snippet() {
        let src = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n";
        let junk = vec![JunkSpan {
            offset: 14,
            line: 2,
            kind: JunkKind::NodeLine,
        }];
        let errors = junk_errors(&junk);
        let out = render_errors("bad.espf.txt", src, &errors);
        eprintln!("{out}");
        assert!(out.contains("malformed node line"));
        assert!(out.contains("bad.espf.txt:2"));
    }

    #[test]
    fn section_header_junk_gets_its_own_title() {
        let src = "hello world\n# peripherals\n";
        let junk = vec![JunkSpan {
            offset: 0,
            line: 1,
            kind: JunkKind::SectionHeader,
        }];
        let errors = junk_errors(&junk);
        let out = render_errors("bad.espf.txt", src, &errors);
        assert!(out.contains("invalid section header"));
        assert!(out.contains("bad.espf.txt:1"));
    }

    #[test]
    fn span_at_attr_points_at_the_value() {
        let at = "# peripherals\n".len();
        let attr = crate::context::TagAttribute {
            key: "pin",
            op: crate::context::AttrOp::Set,
            key_offset: at + 19,
            val: "notanumber",
            val_offset: at + 22,
        };
        let e = EspfError::at_attr(
            &attr,
            "invalid pin number",
            format!("`{}` is not a 0-255 number", attr.val),
        );
        let out = render_errors(
            "x.espf",
            "# peripherals\n  <gpio id=\"gpio4\" pin=\"notanumber\" />\n",
            &[e],
        );
        eprintln!("{out}");
        assert!(out.contains("invalid pin number"));
        assert!(out.contains("notanumber"));
    }

    #[test]
    fn each_error_is_its_own_group() {
        let junk = vec![
            JunkSpan {
                offset: 0,
                line: 1,
                kind: JunkKind::SectionHeader,
            },
            JunkSpan {
                offset: 5,
                line: 1,
                kind: JunkKind::NodeLine,
            },
        ];
        let errors = junk_errors(&junk);
        let src = "a b\n";
        let out = render_errors("x", src, &errors);
        // Two errors -> two `error:` titles.
        assert_eq!(out.matches("error:").count(), 2);
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

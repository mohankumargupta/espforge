//! `annotate-snippets`-backed diagnostics for ESPF files.
//!
//! Every checkable stage of the pipeline (parser, meta validation, `$`
//! dependency resolution) reports problems as one [`EspfError`] carrying a
//! byte-offset span into the original source plus a title and label. This
//! module renders each error as its own rustc-style snippet group:
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
//! output stays plain so it can be grepped or redirected safely.

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
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

/// Byte offsets of the line that contains `offset`, plus its 1-based number.
fn line_bounds(src: &str, line_starts: &[usize], offset: usize) -> (usize, usize, usize) {
    let idx = line_starts.partition_point(|&start| start <= offset) - 1;
    let line_start = line_starts[idx];
    let line_end = src[line_start..]
        .find('\n')
        .map_or(src.len(), |i| line_start + i);
    (line_start, line_end, idx + 1)
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

/// Output renderer; styled (colored) only when stderr is a real terminal.
fn renderer() -> Renderer {
    use std::io::IsTerminal;
    let base = if std::io::stderr().is_terminal() {
        Renderer::styled()
    } else {
        Renderer::plain()
    };
    base.term_width(100)
}

/// Render each error as its own snippet group (no shared highlighting between
/// unrelated errors, matching the rustc one-error-one-block idiom).
pub fn render_errors(file_path: &str, src: &str, errors: &[EspfError]) -> String {
    let mut out = String::new();
    let starts = line_start_offsets(src);

    for e in errors {
        let (line_start_idx, line_end, line_number) = line_bounds(src, &starts, e.span.start);
        // Show the offending line, folding everything else away.
        let view = src[line_start_idx..line_end].trim_end();
        let col_start = e.span.start.saturating_sub(line_start_idx).min(view.len());
        let col_end = e
            .span
            .end
            .saturating_sub(line_start_idx)
            .max(col_start + 1)
            .min(view.len());

        let title = Level::ERROR.primary_title(e.title.clone());
        let group = Group::with_title(title).element(
            Snippet::source(view)
                .line_start(line_number)
                .path(file_path)
                .fold(true)
                .annotation(
                    AnnotationKind::Primary
                        .span(col_start..col_end)
                        .label(e.label.clone())
                        .highlight_source(true),
                ),
        );
        let rendered = renderer().render(&[group]);
        out.push_str(&rendered);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
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

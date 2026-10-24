//! The human renderer: rustc-style snippets, exactly the output the CLI had
//! before the diagnostic model moved here.

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};

use crate::{Diagnostic, Severity};

/// Render each diagnostic as its own snippet group.
///
/// The primary label's line is shown first; every other label's line follows
/// in label order. Lines are folded to what each snippet annotates, and the
/// group keeps one `error:`/`warning:` title, so a two-location diagnostic
/// (a conflict, say) reads as one problem with two underlines.
pub fn to_human(file_path: &str, src: &str, diagnostics: &[Diagnostic]) -> String {
    let starts = line_start_offsets(src);
    let mut out = String::new();

    for d in diagnostics {
        let group = render_group(file_path, src, &starts, d);
        let rendered = renderer().render(&[group]);
        out.push_str(&rendered);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if let Some(help) = &d.help {
            out.push_str(&format!("help: {help}\n"));
        }
    }

    out
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

fn render_group<'a>(
    file_path: &'a str,
    src: &'a str,
    starts: &[usize],
    d: &'a Diagnostic,
) -> Group<'a> {
    let level = match d.severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
        Severity::Note => Level::NOTE,
    };
    let mut group = Group::with_title(level.primary_title(d.message.clone()));

    // The primary line first, then every other annotated line in label order.
    let primary_line = line_bounds(src, starts, d.primary.start).0;
    let mut lines = vec![primary_line];
    for label in &d.labels {
        let start = line_bounds(src, starts, label.span.start).0;
        if !lines.contains(&start) {
            lines.push(start);
        }
    }

    for line_start in lines {
        let (_, line_end, line_number) = line_bounds(src, starts, line_start);
        let view = src[line_start..line_end].trim_end();
        let mut snippet = Snippet::source(view)
            .line_start(line_number)
            .path(file_path)
            .fold(true);

        let mut annotated = false;
        for label in &d.labels {
            let label_line = line_bounds(src, starts, label.span.start).0;
            if label_line != line_start {
                continue;
            }
            let (col_start, col_end) = columns(label.span.start, label.span.end, line_start, view);
            let annotation = if label.span == d.primary {
                AnnotationKind::Primary
                    .span(col_start..col_end)
                    .label(label.text.clone())
                    .highlight_source(true)
            } else {
                AnnotationKind::Context
                    .span(col_start..col_end)
                    .label(label.text.clone())
            };
            snippet = snippet.annotation(annotation);
            annotated = true;
        }

        // No label matched the primary span: underline it with no text rather
        // than losing the location.
        if !annotated && line_start == primary_line {
            let (col_start, col_end) = columns(d.primary.start, d.primary.end, line_start, view);
            snippet = snippet.annotation(
                AnnotationKind::Primary
                    .span(col_start..col_end)
                    .highlight_source(true),
            );
        }

        group = group.element(snippet);
    }

    group
}

/// Columns (relative to `view`) for a span that starts on this line.
fn columns(span_start: usize, span_end: usize, line_start: usize, view: &str) -> (usize, usize) {
    let col_start = span_start.saturating_sub(line_start).min(view.len());
    let col_end = span_end
        .saturating_sub(line_start)
        .max(col_start + 1)
        .min(view.len());
    (col_start, col_end)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagCode, Edit, Fix};

    const SRC: &str = "# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" />\n  <led id=\"other\" gpio=\"$gpio18\" />\n";

    #[test]
    fn one_label_renders_one_snippet_group() {
        let start = SRC.find("<led id=\"other\"").unwrap();
        let d = Diagnostic::error(
            start..start + 4,
            "pin conflict",
            "GPIO18 is already claimed",
        );
        let out = to_human("blink.espf", SRC, &[d]);

        assert_eq!(out.matches("error:").count(), 1, "{out}");
        assert!(out.contains("blink.espf:5"), "{out}");
        assert!(out.contains("GPIO18 is already claimed"), "{out}");
    }

    #[test]
    fn two_labels_on_two_lines_keep_one_title() {
        let first = SRC.find("<gpio id=\"gpio18\"").unwrap();
        let second = SRC.find("<led id=\"other\"").unwrap();
        let d = Diagnostic::error(
            first..first + 5,
            "pin conflict",
            "<gpio id=\"gpio18\"> declares GPIO18 here",
        )
        .with_label(second..second + 4, "<led id=\"other\"> claims it here");

        let out = to_human("blink.espf", SRC, &[d]);

        assert_eq!(out.matches("error:").count(), 1, "{out}");
        assert!(out.contains("declares GPIO18 here"), "{out}");
        assert!(out.contains("claims it here"), "{out}");
        assert!(out.contains("blink.espf:2"), "{out}");
        assert!(out.contains("blink.espf:5"), "{out}");
    }

    #[test]
    fn several_labels_on_one_line_share_a_snippet() {
        let src = "a <led> and <button> collide\n";
        let d = Diagnostic::error(2..7, "pin clash", "led uses GPIO9")
            .with_label(12..20, "button also uses GPIO9");
        let out = to_human("x.espf", src, &[d]);

        assert_eq!(out.matches("error:").count(), 1, "{out}");
        assert!(out.contains("led uses GPIO9"), "{out}");
        assert!(out.contains("button also uses GPIO9"), "{out}");
    }

    #[test]
    fn help_renders_after_the_snippet() {
        let d = Diagnostic::error(0..1, "bad", "here").with_help("use another pin");
        let out = to_human("x.espf", "abc\n", &[d]);
        assert!(out.contains("help: use another pin"), "{out}");
    }

    #[test]
    fn a_primary_span_without_a_label_still_underlines_it() {
        let d = Diagnostic {
            code: Some(DiagCode::new("E0701")),
            severity: Severity::Warning,
            message: "heads up".into(),
            primary: 2..5,
            labels: Vec::new(),
            help: None,
            fixes: Vec::new(),
        };
        let out = to_human("x.espf", "abcdef\n", &[d]);
        assert!(out.contains("warning:"), "{out}");
        assert!(out.contains("heads up"), "{out}");
    }

    #[test]
    fn fixes_do_not_change_human_output() {
        let d = Diagnostic::error(0..3, "bad", "here").with_fix(Fix {
            title: "rename it".into(),
            edits: vec![Edit {
                span: 0..3,
                text: "new".into(),
            }],
        });
        let with_fix = to_human("x.espf", "abcdef\n", &[d.clone()]);
        let without = to_human(
            "x.espf",
            "abcdef\n",
            &[Diagnostic {
                fixes: Vec::new(),
                ..d
            }],
        );
        assert_eq!(with_fix, without);
    }
}

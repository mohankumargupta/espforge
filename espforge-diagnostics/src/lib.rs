//! One diagnostic model for the whole pipeline.
//!
//! Before this crate there were five error conventions: `espf::diag::EspfError`
//! (a title, one label, one span), `String` returns in the element wrappers,
//! `Invalid::{Spans, Message}` in the CLI, `anyhow` at the boundaries and bare
//! exit codes. A problem could be a sentence with no location, a snippet with
//! no code, or a `❌` line from a library module.
//!
//! This crate is the destination of extreme.md §3.4: one [`Diagnostic`] with a
//! stable code, a severity, a primary span, any number of labels, optional
//! help and machine-applicable fixes. Renderers live here too, so the CLI
//! prints and the library never does: [`to_human`] reproduces today's snippet
//! output exactly, [`to_json`] is what CI and editors consume.
//!
//! # Codes
//!
//! Codes are five characters: `E` or `W` followed by four digits. The ranges
//! are reserved by stage so two producers cannot pick the same number:
//!
//! | Range | Stage |
//! |-------|-------|
//! | `E01xx` | syntax (parser junk, section headers) |
//! | `E02xx` | `# espforge` core and meta |
//! | `E03xx` | options and their answers |
//! | `E04xx` | `$` graph resolution |
//! | `E05xx` | peripherals (`<gpio>`, `<i2c>`, `<spi>`, `<uart>`) |
//! | `E06xx` | components (`<led>`, `<button>`, `<*_component>`) |
//! | `E07xx` | ownership and conflicts |
//! | `E08xx` | services and driver routing |
//! | `W0xxx` | warnings |
//!
//! `E0999` is the sentinel for a message missing from the table; it is a bug
//! if it reaches output, and the JSON contract test rejects it.
//!
//! Every diagnostic carries a code. The table lives in [`codes`], one entry
//! per message, so a stage cannot invent a number another stage already uses.
//! The human renderer ignores the code; [`to_json`] is where it is a contract.

use std::ops::Range;

pub mod codes;
pub mod json;
pub mod render;

pub use codes::for_message;

pub use json::to_json;
pub use render::to_human;

/// A byte range into the original source.
pub type Span = Range<usize>;

/// The whole set of problems from one stage, in render order.
pub type Diagnostics = Vec<Diagnostic>;

/// How loudly a diagnostic is reported. Only errors make a command fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    /// The lowercase spelling used by the machine format.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }
}

/// A stable diagnostic code (`E0701`).
///
/// The format is `E`/`W` plus four digits; the ranges are documented on the
/// crate. Codes are a public contract once they reach [`to_json`], so never
/// reuse one for a different message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagCode(&'static str);

impl DiagCode {
    /// A code with the `E`/`W` + four-digit spelling.
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }

    /// The code as it appears in machine output.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// One underline and its explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub text: String,
}

/// One text replacement. Spans are byte offsets into the original source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub span: Span,
    pub text: String,
}

/// A suggested edit, shown as one action (`espforge fix`, an LSP quick-fix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<Edit>,
}

/// One located problem.
///
/// `labels` carries the underline text: the first label whose span equals
/// [`Diagnostic::primary`] is rendered as the primary annotation, every other
/// label as context. A diagnostic with no matching label still underlines the
/// primary span, with no text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable code for this message (see [`codes`]).
    pub code: DiagCode,
    pub severity: Severity,
    /// Short summary on the `error:` line (today's `EspfError::title`).
    pub message: String,
    /// Where the diagnostic points.
    pub primary: Span,
    /// Underline text, primary label included.
    pub labels: Vec<Label>,
    pub help: Option<String>,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    /// An error with a primary span and one label.
    pub fn error(
        code: DiagCode,
        primary: Span,
        message: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            labels: vec![Label {
                span: primary.clone(),
                text: label.into(),
            }],
            primary,
            help: None,
            fixes: Vec::new(),
        }
    }

    /// A warning with a primary span and one label.
    pub fn warning(
        code: DiagCode,
        primary: Span,
        message: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, primary, message, label)
        }
    }

    /// Add a context underline.
    pub fn with_label(mut self, span: Span, text: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            text: text.into(),
        });
        self
    }

    /// Add a `help:` line.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Add a machine-applicable fix.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// The label rendered as the primary annotation, if one matches the
    /// primary span exactly.
    pub fn primary_label(&self) -> Option<&Label> {
        self.labels.iter().find(|label| label.span == self.primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diagnostic_has_one_primary_label() {
        let d = Diagnostic::error(codes::GPIO_INVALID_PIN, 4..8, "bad pin", "no GPIO99")
            .with_label(20..24, "declared here");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.primary_label().unwrap().text, "no GPIO99");
        assert_eq!(d.labels.len(), 2);
    }

    #[test]
    fn codes_are_data_not_prefixes() {
        let d = Diagnostic::error(DiagCode::new("E0701"), 0..1, "boom", "here");
        assert_eq!(d.code.as_str(), "E0701");
    }

    #[test]
    fn the_machine_severity_spelling_is_stable() {
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Note.as_str(), "note");
    }
}

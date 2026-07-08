//! Value objects shared across the pipeline.
//!
//! These are deliberately small, owned, `Copy`/`Clone` types with no business
//! logic — they exist to make invalid states unrepresentable (ADR-005 principle
//! 5) and to carry provenance (source spans) for diagnostics (ADR-009).

use serde::{Deserialize, Serialize};
use std::fmt;

/// A byte-offset span into a source file, used to render span-aware diagnostics
/// (ADR-009). We track byte offsets rather than line/col because the YAML
/// front-end can cheaply produce offsets and the CLI converts to line:col for
/// display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Span {
    /// 0-based byte offset of the start of the node.
    pub start: usize,
    /// 0-based byte offset one-past-the-end of the node.
    pub end: usize,
    /// Index into the set of source files (0 = the project YAML). Reserved for
    /// future multi-file inputs; always 0 today.
    pub file: u16,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end, file: 0 }
    }
}

/// A reference to a named hardware resource or instance in the project.
///
/// `$name` in YAML is normalized to the inner `name` (the leading `$` is
/// stripped at deserialization, ADR-004). The `Span` records where the reference
/// was written so diagnostics can point at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef {
    /// Normalized name without the `$` prefix.
    pub name: String,
    /// Where this reference was written in source.
    pub span: Span,
}

impl ResourceRef {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self { name: name.into(), span }
    }

    /// Construct a ref with no source span (e.g. synthesized internally).
    pub fn synthetic(name: impl Into<String>) -> Self {
        Self { name: name.into(), span: Span::default() }
    }
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.name)
    }
}

/// A reference to a GPIO pin on the target chip. Pins are peripheral-level
/// resources declared in the `esp32:` section (ADR-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinRef {
    /// GPIO number, e.g. 18 for `GPIO18`.
    pub number: u32,
    /// Where this reference was written in source.
    pub span: Span,
}

/// An in-memory generated file. Emitters return `Artifact`s; a thin I/O layer
/// writes them to disk and records them in the ownership manifest (ADR-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Repository-relative path, e.g. `src/generated.rs` or `Cargo.toml`.
    pub path: String,
    /// File contents.
    pub content: String,
    /// `Owned` — espforge regenerates it every build and tracks it in the
    /// manifest (drift detection). `SeedOnce` — espforge writes it only if
    /// absent (the user-owned `app.rs` skeleton); never tracked, never clobbered.
    /// `NotOwned` — not written by espforge at all (e.g. a user override base we
    /// do not emit).
    pub ownership: Ownership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Owned,
    SeedOnce,
    NotOwned,
}

impl Artifact {
    pub fn owned(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self { path: path.into(), content: content.into(), ownership: Ownership::Owned }
    }
    pub fn seed_once(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self { path: path.into(), content: content.into(), ownership: Ownership::SeedOnce }
    }
}

/// A structured, span-aware diagnostic (ADR-009). Replaces bare `anyhow` strings
/// for *config* errors. `anyhow` remains reserved for pipeline/I/O failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// Severity level.
    pub level: Level,
    /// The source span the diagnostic refers to (if known).
    pub span: Option<Span>,
    /// The dotted field path within the document, e.g. `devices.oled.address`.
    pub field_path: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Optional fix hint shown to the user.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

impl Diag {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            span: None,
            field_path: None,
            message: message.into(),
            hint: None,
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn field(mut self, path: impl Into<String>) -> Self {
        self.field_path = Some(path.into());
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Render as a single-line, user-facing message including file:line:col when
    /// the original source text is supplied for line computation.
    pub fn render(&self, source: &str) -> String {
        let loc = self
            .span
            .map(|s| location_label(s, source))
            .unwrap_or_else(|| "espforge".to_string());
        let field = self
            .field_path
            .as_ref()
            .map(|f| format!(" [{f}]"))
            .unwrap_or_default();
        let mut out = format!("{loc}{field}: {}", self.message);
        if let Some(hint) = &self.hint {
            out.push_str(&format!("\n    hint: {hint}"));
        }
        out
    }
}

/// Convert a byte `Span` into a `line:col` label for display.
fn location_label(span: Span, source: &str) -> String {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, b) in source.bytes().enumerate() {
        if i == span.start {
            break;
        }
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    format!("line {line}:{col}")
}

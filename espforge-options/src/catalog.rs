//! The element catalog behind `espforge espf` (KDL v2).
//!
//! The data is generated from `espforge-espgenerate-template/src/espf.schema.kdl`
//! into `crate::generated_catalog` at build time; [`embedded`] hands it out, and
//! nothing is parsed at runtime. Types, the loader (the oracle the generated
//! table is tested against), path resolution and the lookups shared by every
//! view live here; nothing prints — `espforge/src/render.rs` renders,
//! `espforge/src/commands/espf.rs` does I/O.
//!
//! ```kdl
//! section "peripherals" {
//!     summary "The chip's hardware blocks you use"
//!     description "The chip's hardware blocks that your project uses."
//! }
//! element "i2c" section="peripherals" {
//!     summary "I2C bus controller"
//!     attr "sda" kind="pin" required=#true
//!     example "minimal" #"<i2c id="i2c0" sda="6" scl="7" />"#
//!     see_also "components/i2c_component"
//! }
//! ```

use espf::diag::EspfError;
use espforge_espgenerate_template::OVERVIEW_TXT;
use kdl::{KdlDocument, KdlEntry, KdlNode};
use std::ops::Range;
use std::sync::LazyLock;

/// Which chips an element applies to. Reuses the schema's `Compat`.
pub use crate::schema::Compat;

// ---------------------------------------------------------------- data model

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub sections: Vec<SectionDef>,
    /// The prose of `espforge espf overview` (`overview.txt`).
    pub overview: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionDef {
    pub name: String,
    /// One line, shown in the root list.
    pub summary: String,
    /// Paragraph shown above the section's leaf list (terminal only).
    pub description: String,
    pub leafs: Vec<ElementDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDef {
    pub tag: String,
    pub section: String,
    pub summary: String,
    pub description: Option<String>,
    pub status: Status,
    pub attrs: Vec<AttrDef>,
    pub examples: Vec<Example>,
    /// Targets of `see_also`, as `section/tag`.
    pub see_also: Vec<String>,
    pub compat: Compat,
    /// Span of the `element` node in espf.schema.kdl, for loader diagnostics.
    pub span: Range<usize>,
}

impl ElementDef {
    /// `# peripherals` … references to this element in the man page.
    pub fn heading(&self) -> String {
        format!("{}/{}", self.section, self.tag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    /// Printed as the trailing `// title` on the snippet's first element line.
    pub title: String,
    /// One or more lines of valid ESPF (may include `# section` headers).
    pub fragment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrDef {
    /// Dotted names allowed: `network1.ssid`.
    pub name: String,
    pub kind: AttrKind,
    pub required: bool,
    pub default: Option<String>,
    pub help: Option<String>,
    /// Span of the `attr` node in espf.schema.kdl.
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrKind {
    Ident,
    Str,
    Pin,
    Freq,
    Hex,
    U32,
    Bool,
    Choice(Vec<String>),
    /// A `$id` reference to elements of these tags.
    Ref { to: Vec<String> },
    /// A GPIO number or a reference to a `<gpio>`: `cs="9"`, `cs="gpio5"`,
    /// `cs="$gpio5"`.
    PinOrRef,
    /// Repeated with `+=` on every entry, the first one included.
    List(Box<AttrKind>),
}

impl AttrKind {
    /// The short form shown in man pages (`pin`, `ref to i2c`, `none | even`).
    pub fn display(&self) -> String {
        match self {
            AttrKind::Ident => "ident".into(),
            AttrKind::Str => "string".into(),
            AttrKind::Pin => "pin".into(),
            AttrKind::Freq => "freq".into(),
            AttrKind::Hex => "hex".into(),
            AttrKind::U32 => "u32".into(),
            AttrKind::Bool => "bool".into(),
            AttrKind::Choice(c) => c.join(" | "),
            AttrKind::Ref { to } => format!("ref to {}", to.join(", ")),
            AttrKind::PinOrRef => "pin or ref".into(),
            AttrKind::List(inner) => format!("list of {}", inner.display()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Parses, validates and builds today.
    Implemented,
    /// Documented and used by examples, but not buildable yet.
    Planned,
}

impl Status {
    fn parse(s: &str) -> Option<Status> {
        match s {
            "implemented" => Some(Status::Implemented),
            "planned" => Some(Status::Planned),
            _ => None,
        }
    }

    /// Marker shown in section lists next to planned leafs (` (planned)`).
    pub fn marker(&self) -> &'static str {
        match self {
            Status::Implemented => "",
            Status::Planned => " (planned)",
        }
    }
}

impl Catalog {
    pub fn section_named(&self, name: &str) -> Option<&SectionDef> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Every leaf whose tag matches, across all sections. Tags are unique
    /// (checked by the loader), so this has at most one entry.
    pub fn leafs_by_tag(&self, tag: &str) -> Vec<&ElementDef> {
        self.sections
            .iter()
            .flat_map(|s| s.leafs.iter())
            .filter(|e| e.tag == tag)
            .collect()
    }

    /// Which section a tag lives in, if any (for the wrong-section hint).
    fn section_of_tag(&self, tag: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| s.leafs.iter().any(|e| e.tag == tag))
            .map(|s| s.name.as_str())
    }

    /// Section and leaf tags that contain or sound like `input`, for
    /// "did you mean" lines.
    pub fn suggestions(&self, input: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .sections
            .iter()
            .map(|s| s.name.clone())
            .chain(
                self.sections
                    .iter()
                    .flat_map(|s| s.leafs.iter().map(|e| e.tag.clone())),
            )
            .collect();
        names.push("overview".to_string());
        names.sort();
        names.dedup();

        let low = input.to_lowercase();
        // Adjacent-letter transpositions (lde → led) cost distance 2, so
        // short names still get a match.
        let max = match low.len() {
            0..=2 => 1,
            _ => 2,
        };
        let mut scored: Vec<(u32, &str)> = names
            .iter()
            .map(|n| (distance(&low, &n.to_lowercase()), n.as_str()))
            .filter(|(d, _)| *d <= max)
            .collect();
        scored.sort_by_key(|(d, n)| (*d, *n));
        scored.into_iter().map(|(_, n)| n.to_string()).take(4).collect()
    }

    /// Elements (with the referring attribute name) whose `ref` attributes
    /// point at `tag`. Computed, never hand-written: the man page's
    /// `REFERENCED BY` block.
    pub fn referenced_by<'a>(&'a self, tag: &str) -> Vec<(&'a ElementDef, &'a str)> {
        self.sections
            .iter()
            .flat_map(|s| s.leafs.iter())
            .filter_map(|e| {
                let mut via = None;
                for a in &e.attrs {
                    if let AttrKind::Ref { to } = &a.kind {
                        if to.iter().any(|t| t == tag) {
                            via.get_or_insert(a.name.as_str());
                        }
                    }
                }
                via.map(|attr| (e, attr))
            })
            .collect()
    }

    /// Names of every element in every section, for drift-style checks.
    pub fn tags(&self) -> Vec<&str> {
        self.sections
            .iter()
            .flat_map(|s| s.leafs.iter().map(|e| e.tag.as_str()))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// No trailing `help` / `examples` word.
    Default,
    Help,
    Examples,
}

/// Where a resolved catalog path landed, after the view word is stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match<'c> {
    Root(&'c Catalog),
    Overview(&'c Catalog),
    Section(&'c SectionDef),
    Leaf(&'c ElementDef),
}

/// Why the path did not resolve (plan 3.3 rules 3-5). Rendered by
/// `crate::render::render_resolution_error`, never printed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionError {
    /// Unknown name, with close matches for a "did you mean" line.
    Unknown { text: String, suggestions: Vec<String> },
    /// A shorthand leaf name matched several sections. Lists the
    /// `section leaf` forms that work.
    Ambiguous { tag: String, candidates: Vec<String> },
    /// A leaf that exists in a different section: "`i2c` is in `peripherals`".
    WrongSection { tag: String, section: String },
    /// More than two path words, or words after `overview`.
    TooManyArgs { args: Vec<String> },
}

// -------------------------------------------------------------------- loader

/// The catalog behind `espforge espf`, generated from
/// `espforge-espgenerate-template/src/espf.schema.kdl` by this crate's `build.rs`.
///
/// Nothing is parsed at runtime. [`load`] stays the authority on what the schema
/// means: `the_generated_catalog_matches_the_loader` asserts that the generated
/// table equals `load(SCHEMA_KDL)`, so a transcriber mistake and a schema mistake
/// both fail the build. The table is built once, on first use.
pub fn embedded() -> &'static Catalog {
    static CATALOG: LazyLock<Catalog> = LazyLock::new(crate::generated_catalog::catalog);
    &CATALOG
}

/// Names no section or leaf may have (plan 3.3 rule 7).
pub const RESERVED_NAMES: &[&str] = &["help", "examples", "overview"];

fn node_range(n: &KdlNode) -> Range<usize> {
    let s = n.span();
    s.offset()..s.offset() + s.len()
}

fn entry_range(e: &KdlEntry) -> Range<usize> {
    let s = e.span();
    s.offset()..s.offset() + s.len()
}

fn err(span: Range<usize>, title: impl Into<String>, label: impl Into<String>) -> EspfError {
    EspfError::at_span(span, title, label)
}

/// Positional (unnamed) string arguments of a node, with their spans.
fn positional_strings(node: &KdlNode, errors: &mut Vec<EspfError>) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    for e in node.entries().iter().filter(|e| e.name().is_none()) {
        match e.value().as_string() {
            Some(s) => out.push((s.to_string(), entry_range(e))),
            None => errors.push(err(
                entry_range(e),
                format!("`{}` takes strings", node.name().value()),
                "write this argument as a quoted string",
            )),
        }
    }
    out
}

/// Values of a named property like `section=` or `kind=`, with spans.
fn props(node: &KdlNode, key: &str, errors: &mut Vec<EspfError>) -> Vec<(String, Range<usize>)> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_some_and(|n| n.value() == key))
        .map(|e| match e.value().as_string() {
            Some(s) => (s.to_string(), entry_range(e)),
            None => {
                errors.push(err(
                    entry_range(e),
                    format!("`{key}` must be a string"),
                    "quote it",
                ));
                (String::new(), entry_range(e))
            }
        })
        .collect()
}

/// The first value of a named property, flagging repeats as errors.
fn prop<'a>(
    node: &KdlNode,
    key: &'static str,
    errors: &mut Vec<EspfError>,
) -> Option<(String, Range<usize>)> {
    props(node, key, errors).into_iter().next()
}

/// Parse `espf.schema.kdl` text. All problems are reported, each with the byte
/// span of the offending KDL node or argument. Checks include reserved names,
/// tag uniqueness across sections, and every `to=` and `see_also` target.
pub fn load(src: &str) -> Result<Catalog, Vec<EspfError>> {
    let doc = match KdlDocument::parse_v2(src) {
        Ok(d) => d,
        Err(e) => {
            let errs: Vec<EspfError> = e
                .diagnostics
                .iter()
                .map(|d| {
                    let s = d.span;
                    err(
                        s.offset()..s.offset() + s.len(),
                        "invalid KDL",
                        d.message.as_deref().unwrap_or("syntax error"),
                    )
                })
                .collect();
            return Err(if errs.is_empty() {
                vec![err(0..0, "invalid KDL", "the file is not valid KDL v2")]
            } else {
                errs
            });
        }
    };

    let mut sections = Vec::new();
    let mut errors = Vec::new();

    for node in doc.nodes() {
        match node.name().value() {
            "section" => parse_section(node, &mut sections, &mut errors),
            "element" => parse_element(node, &mut sections, &mut errors),
            // The build-option schema lives in the same file;
            // `schema::load` reads it.
            "required" | "options" => {}
            other => errors.push(err(
                node_range(node),
                format!("unknown top-level node `{other}`"),
                "expected `section`, `element`, `required` or `options`",
            )),
        }
    }

    check_references(&sections, &mut errors);
    check_reserved_and_collisions(&sections, &mut errors);

    if errors.is_empty() {
        Ok(Catalog {
            sections,
            overview: OVERVIEW_TXT,
        })
    } else {
        errors.sort_by_key(|e| e.span.start);
        Err(errors)
    }
}

fn parse_section(node: &KdlNode, sections: &mut Vec<SectionDef>, errors: &mut Vec<EspfError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "section has no name", "write `section \"name\" ...`"));
        return;
    }
    let (name, name_span) = names.remove(0);
    if sections.iter().any(|s| s.name == name) {
        errors.push(err(
            name_span,
            format!("duplicate section `{name}`"),
            "each section may be defined once",
        ));
        return;
    }

    let mut summary = None;
    let mut description = None;
    let mut already = false;
    for child in node.children().map(|c| c.nodes()).unwrap_or(&[]) {
        match child.name().value() {
            "summary" | "description" => {
                let is_summary = child.name().value() == "summary";
                if is_summary && already {
                    errors.push(err(
                        node_range(child),
                        "`summary` given twice",
                        "keep one",
                    ));
                    continue;
                }
                if is_summary {
                    already = true;
                }
                let value = positional_strings(child, errors).into_iter().next().map(|(s, _)| s);
                if is_summary {
                    summary = value;
                } else if description.is_some() {
                    errors.push(err(node_range(child), "`description` given twice", "keep one"));
                } else {
                    description = value;
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in section `{name}`"),
                "expected `summary` or `description`",
            )),
        }
    }

    for (taken, what) in [(&summary, "summary"), (&description, "description")] {
        if taken.is_none() {
            errors.push(err(
                span.clone(),
                format!("section `{name}` has no {what}"),
                "write it as a quoted string inside the section",
            ));
        }
    }

    sections.push(SectionDef {
        name,
        summary: summary.unwrap_or_default(),
        description: description.unwrap_or_default(),
        leafs: Vec::new(),
    });
}

/// Parse an `element "tag" section="name"` node into its section's `leafs`.
fn parse_element(node: &KdlNode, sections: &mut Vec<SectionDef>, errors: &mut Vec<EspfError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "element has no tag", "write `element \"tag\" ...`"));
        return;
    }
    let (tag, tag_span) = names.remove(0);
    if RESERVED_NAMES.contains(&tag.as_str()) {
        errors.push(err(
            tag_span,
            format!("`{tag}` is a reserved name"),
            "an element cannot be called help, examples or overview",
        ));
        return;
    }

    let Some((section_name, section_span)) = prop(node, "section", errors) else {
        errors.push(err(
            span,
            format!("element `{tag}` has no section"),
            "add `section=\"name\"` naming a section",
        ));
        return;
    };
    if tag_is_taken(&tag, &*sections) {
        errors.push(err(
            tag_span,
            format!("duplicate tag `{tag}`"),
            "tags are unique across sections, so `espforge espf <tag>` is unambiguous",
        ));
        return;
    }
    let Some(section_index) = sections.iter().position(|s| s.name == section_name) else {
        errors.push(err(
            section_span,
            format!("element `{tag}` names unknown section `{section_name}`"),
            "define that `section` node first, or fix the name",
        ));
        return;
    };

    let mut status = Status::Implemented;
    for (s, s_span) in props(node, "status", errors) {
        match Status::parse(&s) {
            Some(parsed) => status = parsed,
            None => errors.push(err(
                s_span,
                format!("unknown status \"{s}\""),
                "expected \"implemented\" or \"planned\"",
            )),
        }
    }

    let mut summary = None;
    let mut description = None;
    let mut attrs: Vec<AttrDef> = Vec::new();
    let mut examples: Vec<Example> = Vec::new();
    let mut see_also: Vec<String> = Vec::new();
    let mut compat = Compat::default();

    for child in node.children().map(|c| c.nodes()).unwrap_or(&[]) {
        match child.name().value() {
            "summary" | "description" => {
                let slot = if child.name().value() == "summary" {
                    &mut summary
                } else {
                    &mut description
                };
                let value = positional_strings(child, errors).into_iter().next().map(|(s, _)| s);
                if slot.is_some() {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` given twice", child.name().value()),
                        "keep one",
                    ));
                } else if value.is_none() {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` takes a string", child.name().value()),
                        "quote it",
                    ));
                } else {
                    *slot = value;
                }
            }
            "attr" => parse_attr(child, &mut attrs, errors),
            "example" => parse_example(child, &mut examples, errors),
            "see_also" => {
                for (target, s) in positional_strings(child, errors) {
                    if target.split_once('/').is_none() {
                        errors.push(err(
                            s,
                            format!("`{target}` is not a `section/leaf` target"),
                            "write for example `components/i2c_component`",
                        ));
                        continue;
                    }
                    see_also.push(target);
                }
            }
            "compatible" => {
                let mut chips = Vec::new();
                for c in child.children().map(|c| c.nodes()).unwrap_or(&[]) {
                    if c.name().value() == "chip" {
                        for (chip, _) in positional_strings(c, errors) {
                            chips.push(chip);
                        }
                    } else {
                        errors.push(err(
                            node_range(c),
                            format!("unknown node `{}` in `compatible`", c.name().value()),
                            "expected `chip \"name\" ...`",
                        ));
                    }
                }
                compat.chips = Some(chips);
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in element `{tag}`"),
                "expected summary, description, attr, example, see_also or compatible",
            )),
        }
    }

    if summary.is_none() {
        errors.push(err(
            span.clone(),
            format!("element `{tag}` has no summary"),
            "write `summary \"...\"`",
        ));
    }
    if examples.is_empty() {
        errors.push(err(
            span.clone(),
            format!("element `{tag}` has no example"),
            "every element carries at least one `example \"title\" \"fragment\"`",
        ));
    }

    sections[section_index]
        .leafs
        .push(ElementDef {
        tag,
        section: section_name,
        summary: summary.unwrap_or_default(),
        description,
        status,
        attrs,
        examples,
        see_also,
        compat,
        span,
    });
}

fn tag_is_taken(tag: &str, sections: &[SectionDef]) -> bool {
    sections.iter().any(|s| s.leafs.iter().any(|e| e.tag == tag))
}

fn parse_example(node: &KdlNode, examples: &mut Vec<Example>, errors: &mut Vec<EspfError>) {
    let strings = positional_strings(node, errors);
    if strings.len() == 1 {
        errors.push(err(
            node_range(node),
            "`example` takes title and fragment",
            "write `example \"title\" #\"<tag ... />\"#`",
        ));
        return;
    }
    if strings.len() != 2 {
        return; // positional_strings errored already
    }
    let (title, title_span) = &strings[0];
    if examples.iter().any(|ex| &ex.title == title) {
        errors.push(err(
            title_span.clone(),
            format!("duplicate example title `{title}`"),
            "give each example a different title",
        ));
        return;
    }
    examples.push(Example {
        title: title.clone(),
        fragment: strings[1].0.clone(),
    });
}

fn parse_attr(node: &KdlNode, attrs: &mut Vec<AttrDef>, errors: &mut Vec<EspfError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "attr has no name", "write `attr \"name\" ...`"));
        return;
    }
    let (name, name_span) = names.remove(0);
    if attrs.iter().any(|a| a.name == name) {
        errors.push(err(
            name_span,
            format!("duplicate attribute `{name}`"),
            "each attribute may be declared once per element",
        ));
        return;
    }

    let Some((kind_name, kind_span)) = prop(node, "kind", errors) else {
        errors.push(err(span, "attr has no kind", "add `kind=\"..\"`"));
        return;
    };

    let mut required = false;
    let mut required_given = false;
    for e in node
        .entries()
        .iter()
        .filter(|e| e.name().is_some_and(|n| n.value() == "required"))
    {
        if required_given {
            errors.push(err(
                entry_range(e),
                "`required` given twice",
                "keep one",
            ));
        }
        required_given = true;
        match e.value().as_bool() {
            Some(b) => required = b,
            None => errors.push(err(
                entry_range(e),
                "`required` must be #true or #false",
                "KDL v2 booleans are #true / #false",
            )),
        }
    }

    let default = props(node, "default", errors).into_iter().next().map(|(d, _)| d);

    let mut choices: Vec<String> = Vec::new();
    let mut help = None;
    for child in node.children().map(|c| c.nodes()).unwrap_or(&[]) {
        match child.name().value() {
            "choices" => {
                for (c, _) in positional_strings(child, errors) {
                    choices.push(c);
                }
            }
            "help" => match positional_strings(child, errors).into_iter().next() {
                Some((h, _)) => help = Some(h),
                None => errors.push(err(
                    node_range(child),
                    "`help` takes a string",
                    "quote it",
                )),
            },
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in attribute `{name}`"),
                "expected `choices` or `help`",
            )),
        }
    }
    let mut to: Vec<(String, Range<usize>)> = props(node, "to", errors);
    let of = props(node, "of", errors).into_iter().next();

    let kind = build_kind(&name, &kind_name, kind_span, choices, &mut to, of, errors);
    let _ = to;

    attrs.push(AttrDef {
        name,
        kind,
        required,
        default,
        help,
        span,
    });
}

/// Turn the declared pieces into an `AttrKind`, erroring on the shape that
/// does not fit (choice without choices, ref without `to=`, list without
/// `of=`).
fn build_kind(
    name: &str,
    kind_name: &str,
    kind_span: Range<usize>,
    choices: Vec<String>,
    to: &mut [(String, Range<usize>)],
    of: Option<(String, Range<usize>)>,
    errors: &mut Vec<EspfError>,
) -> AttrKind {
    match kind_name {
        "ident" => AttrKind::Ident,
        "str" => AttrKind::Str,
        "pin" => AttrKind::Pin,
        "freq" => AttrKind::Freq,
        "hex" => AttrKind::Hex,
        "u32" => AttrKind::U32,
        "bool" => AttrKind::Bool,
        "choice" => {
            if choices.is_empty() {
                errors.push(err(
                    kind_span,
                    format!("choice attribute `{name}` has no choices"),
                    "add `choices \"a\" \"b\"` inside the attr",
                ));
            }
            AttrKind::Choice(choices)
        }
        "pin_or_ref" => AttrKind::PinOrRef,
        "ref" => {
            let targets: Vec<String> = to.iter().map(|(t, _)| t.clone()).collect();
            if targets.is_empty() {
                errors.push(err(
                    kind_span,
                    format!("ref attribute `{name}` has no `to=`"),
                    "name the tags it may point at, for example `to=\"gpio\"`",
                ));
            }
            AttrKind::Ref { to: targets }
        }
        "list" => {
            let Some((of_name, of_span)) = of else {
                errors.push(err(
                    kind_span,
                    format!("list attribute `{name}` has no `of=`"),
                    "write for example `of=\"str\"`",
                ));
                return AttrKind::List(Box::new(AttrKind::Str));
            };
            let inner = match of_name.as_str() {
                "ident" => Some(AttrKind::Ident),
                "str" => Some(AttrKind::Str),
                "pin" => Some(AttrKind::Pin),
                "freq" => Some(AttrKind::Freq),
                "hex" => Some(AttrKind::Hex),
                "u32" => Some(AttrKind::U32),
                "bool" => Some(AttrKind::Bool),
                other => {
                    errors.push(err(
                        of_span,
                        format!("list element kind \"{other}\" is not supported"),
                        "expected ident, str, pin, freq, hex, u32 or bool",
                    ));
                    None
                }
            };
            AttrKind::List(Box::new(inner.unwrap_or(AttrKind::Str)))
        }
        other => {
            errors.push(err(
                kind_span,
                format!("unknown kind \"{other}\""),
                "expected ident, str, pin, freq, hex, u32, bool, choice, ref, pin_or_ref or list",
            ));
            AttrKind::Str
        }
    }
}

/// Every `ref`'s `to=` names a real tag, and every `see_also` target is a
/// real `section/leaf` (drift tests 3 and 4, enforced by the loader).
fn check_references(sections: &[SectionDef], errors: &mut Vec<EspfError>) {
    let all_tags: Vec<&str> = sections
        .iter()
        .flat_map(|s| s.leafs.iter().map(|e| e.tag.as_str()))
        .collect();
    for section in sections {
        for element in &section.leafs {
            for attr in &element.attrs {
                if let AttrKind::Ref { to } = &attr.kind {
                    for target in to {
                        if !all_tags.contains(&target.as_str()) {
                            errors.push(err(
                                attr.span.clone(),
                                format!(
                                    "`{akey}` of `{etag}` points at unknown tag `{target}`",
                                    akey = attr.name,
                                    etag = element.tag
                                ),
                                format!("defined tags: {}", all_tags.join(" ")),
                            ));
                        }
                    }
                }
            }
            for target in &element.see_also {
                let (target_section, target_leaf) = target
                    .split_once('/')
                    .expect("loader required a `/` when collecting");
                let ok = sections.iter().any(|s| {
                    s.name == target_section && s.leafs.iter().any(|e| e.tag == target_leaf)
                });
                if !ok {
                    errors.push(err(
                        element.span.clone(),
                        format!(
                            "`see_also` of `{tag}` points at `{target}`, which does not exist",
                            tag = element.tag
                        ),
                        format!("defined targets: {}", all_tags.join(" ")),
                    ));
                }
            }
        }
    }
}

/// No section or leaf may be called `help`, `examples` or `overview`, and no
/// leaf may share a name with a section (drift test 5).
fn check_reserved_and_collisions(sections: &[SectionDef], errors: &mut Vec<EspfError>) {
    let section_names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
    for section in sections {
        if RESERVED_NAMES.contains(&section.name.as_str()) {
            errors.push(err(
                0..0,
                format!("section name `{}` is reserved", section.name),
                "sections cannot be called help, examples or overview",
            ));
        }
        for element in &section.leafs {
            if RESERVED_NAMES.contains(&element.tag.as_str()) {
                errors.push(err(
                    element.span.clone(),
                    format!("element tag `{}` is reserved", element.tag),
                    "elements cannot be called help, examples or overview",
                ));
            }
            if !section_names.is_empty() && section_names.contains(&element.tag.as_str()) {
                errors.push(err(
                    element.span.clone(),
                    format!("element `{}` shares a name with a section", element.tag),
                    "the shorthand `espforge espf <name>` must stay unambiguous",
                ));
            }
        }
    }
}

// ---------------------------------------------------------------- resolution

/// Rule 1 of plan 3.3: a trailing `help` or `examples` is the view, remembered
/// and stripped before any lookup, so no leaf can be named `help` or
/// `examples`. `overview` is a normal word.
pub fn strip_view(args: Vec<String>) -> (Vec<String>, View) {
    match args.split_last() {
        Some((last, rest)) if last == "help" => (rest.to_vec(), View::Help),
        Some((last, rest)) if last == "examples" => (rest.to_vec(), View::Examples),
        _ => (args, View::Default),
    }
}

/// Resolve a catalog path once, for every view (plan 3.3).
pub fn resolve<'c>(catalog: &'c Catalog, args: &[String]) -> Result<Match<'c>, ResolutionError> {
    if args.len() > 2 {
        return Err(ResolutionError::TooManyArgs {
            args: args.to_vec(),
        });
    }
    match args {
        [] => Ok(Match::Root(catalog)),
        [one] if one == "overview" => Ok(Match::Overview(catalog)),
        [one] => {
            if let Some(section) = catalog.section_named(one) {
                return Ok(Match::Section(section));
            }
            match catalog.leafs_by_tag(one).as_slice() {
                [leaf] => Ok(Match::Leaf(leaf)),
                [] => Err(ResolutionError::Unknown {
                    text: one.clone(),
                    suggestions: catalog.suggestions(one),
                }),
                [..] => Err(ResolutionError::Ambiguous {
                    tag: one.clone(),
                    candidates: catalog
                        .sections
                        .iter()
                        .flat_map(|s| {
                            s.leafs
                                .iter()
                                .filter(|e| e.tag == *one)
                                .map(move |_| format!("{} {}", s.name, one))
                        })
                        .collect(),
                }),
            }
        }
        [section_name, leaf] => {
            if section_name == "overview" {
                return Err(ResolutionError::TooManyArgs {
                    args: args.to_vec(),
                });
            }
            match catalog.section_named(section_name) {
                Some(section) => {
                    if let Some(element) = section.leafs.iter().find(|e| e.tag == *leaf) {
                        return Ok(Match::Leaf(element));
                    }
                    if let Some(other) = catalog.section_of_tag(leaf) {
                        return Err(ResolutionError::WrongSection {
                            tag: leaf.clone(),
                            section: other.to_string(),
                        });
                    }
                    Err(ResolutionError::Unknown {
                        text: leaf.clone(),
                        suggestions: catalog.suggestions(leaf),
                    })
                }
                None => {
                    if let Some(other) = catalog.section_of_tag(leaf) {
                        return Err(ResolutionError::WrongSection {
                            tag: leaf.clone(),
                            section: other.to_string(),
                        });
                    }
                    Err(ResolutionError::Unknown {
                        text: section_name.clone(),
                        suggestions: catalog.suggestions(section_name),
                    })
                }
            }
        }
        // Unreachable: `args.len() > 2` returned above, but the compiler
        // wants the case to make the match total.
        _ => Err(ResolutionError::TooManyArgs {
            args: args.to_vec(),
        }),
    }
}

/// Levenshtein distance on bytes; good enough for close-name suggestions.
fn distance(a: &str, b: &str) -> u32 {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.is_empty() {
        return b.len() as u32;
    }
    if b.is_empty() {
        return a.len() as u32;
    }
    let mut prev: Vec<u32> = (0..=b.len() as u32).collect();
    let mut cur = vec![0u32; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = (i + 1) as u32;
        for (j, &cb) in b.iter().enumerate() {
            let cost = u32::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_espgenerate_template::SCHEMA_KDL;

    const SAMPLE: &str = r##"
section "espforge" {
    summary "The project itself"
    description "Settings for the build as a whole."
}

section "peripherals" {
    summary "The chip's hardware blocks you use"
    description "The chip's hardware blocks that your project uses."
}

element "gpio" section="peripherals" {
    summary "A single GPIO pin"
    attr "id" kind="ident" required=#true
    attr "pin" kind="pin" required=#true {
        help "GPIO number."
    }
    example "minimal" #"<gpio id="gpio4" pin="4" />"#
}

element "led" section="peripherals" status="planned" {
    summary "An LED on a pin"
    attr "id" kind="ident" required=#true
    attr "pin" kind="ref" to="gpio" required=#true
    example "minimal" #"<led id="led0" pin="$gpio4" />"#
}

element "wifi" section="espforge" {
    summary "Wi-Fi station"
    attr "id" kind="ident" required=#true
    attr "network1.ssid" kind="str" required=#true
    attr "servers" kind="list" of="str"
    example "one" #"<wifi id="wifi0" network1.ssid="home" />"#
}
"##;

    #[test]
    fn loads_a_sample_catalog() {
        let c = load(SAMPLE).unwrap();
        assert_eq!(c.sections.len(), 2);
        let peripherals = c.section_named("peripherals").unwrap();
        assert_eq!(peripherals.leafs.len(), 2);
        let led = c.section_named("peripherals").unwrap().leafs[1].clone();
        assert_eq!(led.status, Status::Planned);
        assert_eq!(led.status.marker(), " (planned)");
        let on_gpio = peripheral_attr(&c, "gpio", "pin").kind.clone();
        assert_eq!(on_gpio, AttrKind::Pin);
        let wifi = c.section_named("espforge").unwrap().leafs[0].clone();
        assert_eq!(wifi.attrs[1].name, "network1.ssid");
        assert_eq!(
            wifi.attrs[2].kind,
            AttrKind::List(Box::new(AttrKind::Str))
        );
        let ref_attr = peripheral_attr(&c, "led", "pin").kind.clone();
        assert_eq!(ref_attr, AttrKind::Ref { to: vec!["gpio".into()] });
        // c.offered? -- see schema-based tests in drift module.
    }

    fn peripheral_attr<'c>(c: &'c Catalog, tag: &str, attr: &str) -> &'c AttrDef {
        c.leafs_by_tag(tag)[0]
            .attrs
            .iter()
            .find(|a| a.name == attr)
            .unwrap()
    }

    /// The table `espforge espf` uses is the one `build.rs` wrote, and it is
    /// exactly what the loader reads out of the same file.
    #[test]
    fn the_generated_catalog_matches_the_loader() {
        assert_eq!(
            embedded(),
            &load(SCHEMA_KDL).unwrap(),
            "src/generated_catalog.rs is stale: rebuild espforge-options"
        );
    }

    #[test]
    fn the_embedded_catalog_loads_cleanly() {
        let c = load(SCHEMA_KDL).unwrap();
        for name in ["espforge", "peripherals", "components", "devices", "services"] {
            assert!(c.section_named(name).is_some(), "missing section {name}");
        }
        for tag in ["gpio", "i2c", "spi", "uart", "i2c_component", "status_led", "tmp102"] {
            assert!(!c.leafs_by_tag(tag).is_empty(), "missing element {tag}");
        }
        // The planned leafs from phase 3 land with a marker.
        assert_eq!(c.leafs_by_tag("wifi")[0].status, Status::Planned);
    }

    #[test]
    fn embedded_catalog_has_no_hardcoded_chip_list() {
        // Decision 10 of plan 8: chips, runtimes and consoles come from the
        // existing functions; the catalog only describes the element's shape.
        let c = load(SCHEMA_KDL).unwrap();
        let chip = c.leafs_by_tag("chip").remove(0);
        let mut is_choice = false;
        for attr in &chip.attrs {
            if let AttrKind::Choice(_) = attr.kind {
                is_choice = true;
            }
        }
        assert!(!is_choice, "chip's type must not hardcode chips");
    }

    #[test]
    fn reserved_names_and_collisions_are_errors() {
        let bad = r##"
section "examples" {
    summary "s"
    description "d"
}
element "i2c" section="examples" {
    summary "s"
    example "one" #"<i2c id="i2c0" />"#
}
"##;
        let errs = load(bad).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].title.contains("`examples` is reserved"), "{errs:?}");
    }

    #[test]
    fn tags_are_unique_across_sections() {
        let bad = r##"
section "peripherals" {
    summary "s"
    description "d"
}
section "components" {
    summary "s"
    description "d"
}
element "i2c" section="peripherals" {
    summary "bus"
    example "one" #"<i2c id="i2c0" />"#
}
element "i2c" section="components" {
    summary "bus"
    example "one" #"<i2c id="i2c0" />"#
}
"##;
        let errs = load(bad).unwrap_err();
        assert!(errs[0].title.contains("duplicate tag"), "{errs:?}");
    }

    #[test]
    fn see_also_and_to_targets_must_exist() {
        let bad = r##"
section "peripherals" {
    summary "s"
    description "d"
}
element "i2c" section="peripherals" {
    summary "bus"
    attr "bus" kind="ref" to="nope" required=#true
    example "one" #"<i2c id="i2c0" />"#
    see_also "components/nosuch"
}
"##;
        let errs = load(bad).unwrap_err();
        let titles: Vec<String> = errs.iter().map(|e| e.title.clone()).collect();
        assert!(titles.iter().any(|t| t.contains("unknown tag `nope`")), "{titles:?}");
        assert!(
            titles
                .iter()
                .any(|t| t.contains("when does not exist") || t.contains("which does not exist")),
            "{titles:?}"
        );
    }

    #[test]
    fn summaries_and_examples_are_required() {
        let bad = r##"
section "peripherals" {
    summary "s"
    description "d"
}
element "i2c" section="peripherals" {
    attr "id" kind="ident" required=#true
}
"##;
        let errs = load(bad).unwrap_err();
        let titles: Vec<String> = errs.iter().map(|e| e.title.clone()).collect();
        assert!(titles.iter().any(|t| t.contains("has no summary")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("has no example")), "{titles:?}");
    }

    // --------------------------------------------------------------- resolve

    fn argvec(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn resolve_ok<'c>(catalog: &'c Catalog, args: &[&str]) -> Match<'c> {
        resolve(
            catalog,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .map_err(|e| format!("{e:?}"))
        .unwrap()
    }

    #[test]
    fn strip_view_removes_one_trailing_view_word() {
        let (path, view) = strip_view(vec!["peripherals".into(), "i2c".into(), "help".into()]);
        assert_eq!((path, view), (argvec(&["peripherals", "i2c"]), View::Help));
        let (path, view) = strip_view(vec!["i2c".into(), "examples".into()]);
        assert_eq!((path, view), (vec!["i2c".to_string()], View::Examples));
        let (_, view) = strip_view(vec!["overview".into()]);
        assert_eq!(view, View::Default, "overview is not a view word");
        let (_, view) = strip_view(vec![]);
        assert_eq!(view, View::Default);
    }

    #[test]
    fn all_levels_resolve() {
        let c = load(SAMPLE).unwrap();
        assert_eq!(resolve_ok(&c, &[]), Match::Root(&c));
        assert_eq!(resolve_ok(&c, &["overview"]), Match::Overview(&c));
        assert!(matches!(resolve_ok(&c, &["peripherals"]), Match::Section(_)));
        assert!(matches!(resolve_ok(&c, &["led"]), Match::Leaf(_)));
        assert!(matches!(
            resolve_ok(&c, &["peripherals", "led"]),
            Match::Leaf(_)
        ));
    }

    #[test]
    fn unknown_names_carry_suggestions() {
        let c = load(SAMPLE).unwrap();
        assert_eq!(
            resolve(&c, &argvec(&["nope"])).unwrap_err(),
            ResolutionError::Unknown {
                text: "nope".to_string(),
                suggestions: Vec::new(),
            }
        );
        // Near-miss: `lde` suggests `led`.
        let Err(ResolutionError::Unknown { suggestions, .. }) = resolve(&c, &argvec(&["lde"])) else {
            panic!("expected Unknown");
        };
        assert!(suggestions.contains(&"led".to_string()), "{suggestions:?}");
    }

    #[test]
    fn wrong_section_gets_the_pointed_hint() {
        let c = load(SAMPLE).unwrap();
        assert_eq!(
            resolve(&c, &argvec(&["espforge", "led"])).unwrap_err(),
            ResolutionError::WrongSection {
                tag: "led".to_string(),
                section: "peripherals".to_string(),
            }
        );
    }

    #[test]
    fn overview_and_extra_words_reject_further_arguments() {
        let c = load(SAMPLE).unwrap();
        assert_eq!(
            resolve(&c, &argvec(&["overview", "peripherals"])).unwrap_err(),
            ResolutionError::TooManyArgs {
                args: vec!["overview".into(), "peripherals".into()]
            }
        );
        assert!(matches!(
            resolve(&c, &argvec(&["a", "b", "c"])).unwrap_err(),
            ResolutionError::TooManyArgs { .. }
        ));
    }

    #[test]
    fn suggestions_are_close_matches() {
        let c = load(SAMPLE).unwrap();
        let near = c.suggestions("led");
        assert!(near.iter().any(|n| n == "led"), "{near:?}");
        let none = c.suggestions("zzzzzz");
        // `zzzzzz` is far from every name of a two-section sample.
        assert!(none.iter().all(|n| n != "led"), "{none:?}");
    }

    #[test]
    fn referenced_by_is_computed_from_ref_attributes() {
        let c = load(SAMPLE).unwrap();
        let via = c.referenced_by("gpio");
        assert_eq!(via.len(), 1);
        assert_eq!(via[0].0.tag, "led");
        assert_eq!(via[0].1, "pin");
        assert!(c.referenced_by("led").is_empty());
    }

    #[test]
    fn overview_text_is_carried() {
        let c = load(SCHEMA_KDL).unwrap();
        assert!(c.overview.contains("ESPF"), "overview must be the real page");
    }

    // ------------------------------------------------------- drift tests
    // The catalog is a checked contract (plan section 8): the loader checks
    // references and reserved names (tests 3-5); these check the examples.

    /// A fragment that has no `# section` header (a bare one-liner snippet)
    /// gets its element's own section prepended, so the parser sees a
    /// complete file and both drift tests check everything.
    fn parseable_fragment(section: &str, fragment: &str) -> String {
        let has_header = fragment.lines().any(|l| l.trim_start().starts_with('#'));
        if has_header {
            fragment.to_string()
        } else {
            format!("# {section}\n  {fragment}")
        }
    }

    /// Drift test 1: every example fragment parses with no junk lines.
    /// Missing `# section` headers in a fragment are fine; junk is not.
    #[test]
    fn every_example_fragment_parses_without_junk() {
        let c = load(SCHEMA_KDL).unwrap();
        let mut checked = 0;
        for s in &c.sections {
            for e in &s.leafs {
                for ex in &e.examples {
                    let tag = e.tag.as_str();
                    let title = ex.title.as_str();
                    let text = parseable_fragment(&e.section, &ex.fragment);
                    let parsed = espf::parser::parse_espf(&text);
                    assert!(
                        parsed.junk.is_empty(),
                        "junk lines {:?} in `{tag}` example `{title}`\
                         \nfragment:\n{}",
                        parsed.junk,
                        ex.fragment,
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 20, "the catalog should have examples: {checked}");
    }

    /// Drift test 2: every element in every fragment has a tag in the
    /// catalog, uses only declared attributes, and includes the required
    /// ones.
    #[test]
    fn every_fragment_element_matches_the_catalog() {
        let c = load(SCHEMA_KDL).unwrap();
        for s in &c.sections {
            for e in &s.leafs {
                for ex in &e.examples {
                    let tag = e.tag.as_str();
                    let title = ex.title.as_str();
                    let text = parseable_fragment(&e.section, &ex.fragment);
                    let parsed = espf::parser::parse_espf(&text);
                    for nodes in parsed.sections.values() {
                        for node in nodes {
                            let used = node.tag_name;
                            let def = c
                                .leafs_by_tag(used)
                                .into_iter()
                                .next()
                                .unwrap_or_else(|| {
                                    panic!(
                                        "tag `{used}` in `{tag}` example `{title}` is not in the catalog"
                                    )
                                });
                            let declared: Vec<&str> =
                                def.attrs.iter().map(|a| a.name.as_str()).collect();
                            for attr in &node.attributes {
                                let key = attr.key;
                                assert!(
                                    declared.contains(&key),
                                    "`{tag}` example `{title}`: `{used}` uses attribute `{key}` \
                                     not declared on it\ndeclared: {declared:?}"
                                );
                            }
                            for a in def.attrs.iter().filter(|a| a.required) {
                                assert!(
                                    node.get_attr(&a.name).is_some(),
                                    "`{used}` in `{tag}` example `{title}` misses \
                                     required attribute `{}`",
                                    a.name,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

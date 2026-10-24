//! The option half of `espf.schema.kdl`: types and loader (KDL v2).
//!
//! ```kdl
//! required "chip"
//! options {
//!     option "wifi" display_name="Enable Wi-Fi" {
//!         help "..."
//!         requires "alloc" "embassy"
//!         compatible { chip "esp32" "esp32c3" }
//!     }
//! }
//! ```
//!
//! The same file also holds the element vocabulary (`section`/`element`); this
//! loader skips those nodes and the catalog in [`crate::catalog`] skips these.
//!
//! Option properties: `display_name`, `selection_group`, `requires_nightly`,
//! `kind` (`bool` default, `string`, `choice`, `list`) and `default`.
//! Children: `help`, `requires`, `requires_tools`, `compatible { chip ... }`,
//! `choices` (for `kind="choice"`).

use espf::diag::{EspfError, render_errors};
use kdl::{KdlDocument, KdlEntry, KdlNode};
use std::ops::Range;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptValue {
    Bool(bool),
    Str(String),
    List(Vec<String>),
}

impl OptValue {
    /// "Switched on": `true`, a non-empty string, or a non-empty list.
    pub fn is_on(&self) -> bool {
        match self {
            OptValue::Bool(b) => *b,
            OptValue::Str(s) => !s.is_empty(),
            OptValue::List(l) => !l.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Bool,
    String,
    Choice(Vec<String>),
    List,
}

/// Which chips an option is available for. `None` = all chips.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compat {
    pub chips: Option<Vec<String>>,
}

impl Compat {
    pub fn allows(&self, chip: &str) -> bool {
        self.chips
            .as_ref()
            .is_none_or(|c| c.iter().any(|x| x == chip))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requires {
    /// Other options that must be on when this one is.
    pub options: Vec<String>,
    /// External tools (e.g. `probe-rs`) needed; informational for `doctor`.
    pub tools: Vec<String>,
    pub nightly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDef {
    pub name: String,
    pub display_name: Option<String>,
    pub help: Option<String>,
    pub kind: Kind,
    /// Value used when `defaults="true"` and nothing was given.
    pub default: Option<OptValue>,
    pub requires: Requires,
    pub compat: Compat,
    pub selection_group: Option<String>,
    /// Span of the `option` node in `espf.schema.kdl`.
    pub span: Range<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    /// Answers that must always be provided (`required "chip"`).
    pub required: Vec<String>,
    pub options: Vec<OptionDef>,
}

impl Schema {
    pub fn get(&self, name: &str) -> Option<&OptionDef> {
        self.options.iter().find(|o| o.name == name)
    }
}

/// The schema embedded from `espforge-espgenerate-template/src/espf.schema.kdl`.
/// Panics (with a rendered diagnostic) if the embedded file is invalid; the
/// unit tests make sure it is not.
pub fn embedded_schema() -> &'static Schema {
    static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
        let src = espforge_espgenerate_template::SCHEMA_KDL;
        load(src).unwrap_or_else(|errs| {
            panic!(
                "embedded espf.schema.kdl is invalid:\n{}",
                render_errors("espf.schema.kdl", src, &errs)
            )
        })
    });
    &SCHEMA
}

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

/// Parse the option half of a schema file. `section`/`element` nodes belong to
/// the element catalog and are skipped. All problems are reported, each with
/// the byte span of the offending KDL node or argument.
pub fn load(src: &str) -> Result<Schema, Vec<EspfError>> {
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

    let mut schema = Schema::default();
    let mut errors = Vec::new();
    // (option, required option, span) checked once every option is known.
    let mut requires_refs: Vec<(String, String, Range<usize>)> = Vec::new();

    for node in doc.nodes() {
        match node.name().value() {
            "required" => {
                for (name, _) in positional_strings(node, &mut errors) {
                    schema.required.push(name);
                }
            }
            "options" => {
                for opt in node.children().map(|c| c.nodes()).unwrap_or(&[]) {
                    if opt.name().value() == "option" {
                        parse_option(opt, &mut schema, &mut requires_refs, &mut errors);
                    } else {
                        errors.push(err(
                            node_range(opt),
                            format!("unknown node `{}` in `options`", opt.name().value()),
                            "only `option` nodes are allowed here",
                        ));
                    }
                }
            }
            // The element vocabulary lives in the same file; `catalog::load`
            // reads it.
            "section" | "element" => {}
            other => errors.push(err(
                node_range(node),
                format!("unknown top-level node `{other}`"),
                "expected `section`, `element`, `required` or `options`",
            )),
        }
    }

    for (opt, target, span) in requires_refs {
        if schema.get(&target).is_none() {
            errors.push(err(
                span,
                format!("`{opt}` requires unknown option `{target}`"),
                "no `option` with that name is defined",
            ));
        }
    }

    if errors.is_empty() {
        Ok(schema)
    } else {
        errors.sort_by_key(|e| e.span.start);
        Err(errors)
    }
}

fn parse_option(
    node: &KdlNode,
    schema: &mut Schema,
    requires_refs: &mut Vec<(String, String, Range<usize>)>,
    errors: &mut Vec<EspfError>,
) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "option has no name",
            "write `option \"name\" ...`",
        ));
        return;
    }
    let (name, name_span) = names.remove(0);
    if schema.get(&name).is_some() {
        errors.push(err(
            name_span,
            format!("duplicate option `{name}`"),
            "each option may be defined once",
        ));
        return;
    }

    let mut def = OptionDef {
        name: name.clone(),
        display_name: None,
        help: None,
        kind: Kind::Bool,
        default: None,
        requires: Requires::default(),
        compat: Compat::default(),
        selection_group: None,
        span,
    };
    let mut kind_name = "bool".to_string();
    let mut kind_span = def.span.clone();
    let mut default_entry: Option<&KdlEntry> = None;

    for e in node.entries().iter().filter(|e| e.name().is_some()) {
        let key = e.name().map(|k| k.value()).unwrap_or("");
        let v = e.value();
        match key {
            "display_name" | "selection_group" | "kind" => match v.as_string() {
                Some(s) => match key {
                    "display_name" => def.display_name = Some(s.to_string()),
                    "selection_group" => def.selection_group = Some(s.to_string()),
                    _ => {
                        kind_name = s.to_string();
                        kind_span = entry_range(e);
                    }
                },
                None => errors.push(err(
                    entry_range(e),
                    format!("`{key}` must be a string"),
                    "quote it",
                )),
            },
            "requires_nightly" => match v.as_bool() {
                Some(b) => def.requires.nightly = b,
                None => errors.push(err(
                    entry_range(e),
                    "`requires_nightly` must be #true or #false",
                    "KDL v2 booleans are #true / #false",
                )),
            },
            "default" => default_entry = Some(e),
            other => errors.push(err(
                entry_range(e),
                format!("unknown property `{other}` on option `{name}`"),
                "expected display_name, selection_group, requires_nightly, kind or default",
            )),
        }
    }

    let mut choices: Vec<String> = Vec::new();
    for child in node.children().map(|c| c.nodes()).unwrap_or(&[]) {
        match child.name().value() {
            "help" => {
                if let Some((h, _)) = positional_strings(child, errors).into_iter().next() {
                    def.help = Some(h);
                }
            }
            "requires" => {
                for (target, s) in positional_strings(child, errors) {
                    requires_refs.push((name.clone(), target.clone(), s));
                    def.requires.options.push(target);
                }
            }
            "requires_tools" => {
                for (tool, _) in positional_strings(child, errors) {
                    def.requires.tools.push(tool);
                }
            }
            "choices" => {
                for (c, _) in positional_strings(child, errors) {
                    choices.push(c);
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
                def.compat.chips = Some(chips);
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in option `{name}`"),
                "expected help, requires, requires_tools, compatible or choices",
            )),
        }
    }

    def.kind = match kind_name.as_str() {
        "bool" => Kind::Bool,
        "string" => Kind::String,
        "list" => Kind::List,
        "choice" => {
            if choices.is_empty() {
                errors.push(err(
                    kind_span.clone(),
                    format!("choice option `{name}` has no choices"),
                    "add `choices \"a\" \"b\"` inside the option",
                ));
            }
            Kind::Choice(choices)
        }
        other => {
            errors.push(err(
                kind_span,
                format!("unknown kind \"{other}\""),
                "expected bool, string, choice or list",
            ));
            Kind::Bool
        }
    };

    def.default = match (&def.kind, default_entry) {
        (Kind::Bool, None) => Some(OptValue::Bool(false)),
        (Kind::Bool, Some(e)) => match e.value().as_bool() {
            Some(b) => Some(OptValue::Bool(b)),
            None => {
                errors.push(err(
                    entry_range(e),
                    "bool default must be #true or #false",
                    "KDL v2 booleans are #true / #false",
                ));
                None
            }
        },
        (Kind::String, Some(e)) => match e.value().as_string() {
            Some(s) => Some(OptValue::Str(s.to_string())),
            None => {
                errors.push(err(
                    entry_range(e),
                    "string default must be a string",
                    "quote it",
                ));
                None
            }
        },
        (Kind::Choice(cs), Some(e)) => match e.value().as_string() {
            Some(s) if cs.iter().any(|c| c == s) => Some(OptValue::Str(s.to_string())),
            _ => {
                errors.push(err(
                    entry_range(e),
                    "default is not one of the choices",
                    format!("choices: {}", cs.join(", ")),
                ));
                None
            }
        },
        (Kind::List, Some(e)) => {
            errors.push(err(
                entry_range(e),
                "list options cannot have a default",
                "remove `default=`",
            ));
            None
        }
        (_, None) => None,
    };

    schema.options.push(def);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
required "chip"
options {
    option "a" display_name="Option A" {
        help "does a"
    }
    option "b" requires_nightly=#true selection_group="g" default=#true {
        requires "a"
        requires_tools "probe-rs"
        compatible { chip "esp32c3" "esp32" }
    }
    option "mode" kind="choice" default="fast" {
        choices "fast" "slow"
    }
}
"#;

    #[test]
    fn loads_a_small_schema() {
        let s = load(SAMPLE).unwrap();
        assert_eq!(s.required, ["chip"]);
        assert_eq!(s.options.len(), 3);
        let a = s.get("a").unwrap();
        assert_eq!(a.display_name.as_deref(), Some("Option A"));
        assert_eq!(a.help.as_deref(), Some("does a"));
        assert_eq!(a.default, Some(OptValue::Bool(false)));
        let b = s.get("b").unwrap();
        assert!(b.requires.nightly);
        assert_eq!(b.requires.options, ["a"]);
        assert_eq!(b.requires.tools, ["probe-rs"]);
        assert_eq!(b.selection_group.as_deref(), Some("g"));
        assert_eq!(b.default, Some(OptValue::Bool(true)));
        assert!(b.compat.allows("esp32c3") && !b.compat.allows("esp32s3"));
        assert!(a.compat.allows("anything"));
        let m = s.get("mode").unwrap();
        assert_eq!(m.kind, Kind::Choice(vec!["fast".into(), "slow".into()]));
        assert_eq!(m.default, Some(OptValue::Str("fast".into())));
    }

    #[test]
    fn invalid_kdl_is_reported() {
        assert!(!load("options {").unwrap_err().is_empty());
    }

    #[test]
    fn requires_unknown_option_is_located() {
        let src = "options {\n    option \"a\" {\n        requires \"nope\"\n    }\n}\n";
        let errs = load(src).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].title.contains("unknown option `nope`"));
        assert_eq!(src[errs[0].span.clone()].trim(), "\"nope\"");
    }

    #[test]
    fn duplicate_and_unknown_things_are_errors() {
        let src = "options {\n    option \"a\" bogus=\"x\"\n    option \"a\"\n    widget \"z\"\n}\nextra\n";
        let errs = load(src).unwrap_err();
        let titles: Vec<&str> = errs.iter().map(|e| e.title.as_str()).collect();
        assert!(
            titles
                .iter()
                .any(|t| t.contains("unknown property `bogus`")),
            "{titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("duplicate option `a`")),
            "{titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("unknown node `widget`")),
            "{titles:?}"
        );
        assert!(
            titles
                .iter()
                .any(|t| t.contains("unknown top-level node `extra`")),
            "{titles:?}"
        );
    }

    #[test]
    fn the_embedded_schema_options_load_cleanly() {
        let s = embedded_schema();
        for name in ["wifi", "embassy", "alloc", "unstable-hal", "probe-rs"] {
            assert!(s.get(name).is_some(), "missing {name}");
        }
        assert!(
            s.get("wifi")
                .unwrap()
                .requires
                .options
                .contains(&"embassy".to_string())
        );
    }
}

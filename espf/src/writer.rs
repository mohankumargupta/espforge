//! Canonical serializer for ESPF documents.
//!
//! Output rules (the "canonical form"):
//! * sections in fixed order (`espforge`, `peripherals`, `components`,
//!   `devices`, `services`), all five always emitted, blank line between;
//!   any extra sections follow in name order;
//! * one element per line, indented two spaces;
//! * `id` first, then the remaining keys in sorted order;
//! * every value double-quoted; objects use dot notation (`a.b="x"`),
//!   arrays use `+=` on every entry, in their original order.
//!
//! Round-trip rule: what the writer emits parses back to an equal value
//! tree (spans, comments and attribute order are not part of equality).

use crate::context::ParsedNode;
use crate::diag::EspfError;
use crate::parser::REQUIRED_SECTIONS;
use crate::value::{Object, Value, assemble};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// One element: a tag and its assembled attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: String,
    pub attrs: Object,
}

impl Element {
    pub fn from_node(node: &ParsedNode) -> Result<Self, Vec<EspfError>> {
        Ok(Self {
            tag: node.tag_name.to_string(),
            attrs: assemble(node)?,
        })
    }
}

/// A whole document: section name -> elements in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub sections: BTreeMap<String, Vec<Element>>,
}

impl Document {
    /// Assemble every element of every section, collecting all errors.
    pub fn from_sections(
        sections: &HashMap<String, Vec<ParsedNode>>,
    ) -> Result<Self, Vec<EspfError>> {
        let mut doc = Document::default();
        let mut errors = Vec::new();
        for (name, nodes) in sections {
            let mut els = Vec::with_capacity(nodes.len());
            for node in nodes {
                match Element::from_node(node) {
                    Ok(el) => els.push(el),
                    Err(mut e) => errors.append(&mut e),
                }
            }
            doc.sections.insert(name.clone(), els);
        }
        if errors.is_empty() {
            Ok(doc)
        } else {
            errors.sort_by_key(|e| e.span.start);
            Err(errors)
        }
    }
}

/// The value cannot be represented in ESPF (e.g. a `"` inside a value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteError(pub String);

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WriteError {}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn check_ident(what: &str, s: &str) -> Result<(), WriteError> {
    if is_ident(s) {
        Ok(())
    } else {
        Err(WriteError(format!(
            "{what} `{s}` is not a valid ESPF identifier"
        )))
    }
}

fn quote(val: &str) -> Result<String, WriteError> {
    if val.contains(['"', '\n', '\r']) {
        return Err(WriteError(format!(
            "value {val:?} cannot be written: ESPF strings cannot contain `\"` or line breaks"
        )));
    }
    Ok(format!("\"{val}\""))
}

/// Flatten an object into `(dotted key, operator, value)` triples, sorted.
fn flatten<'a>(
    obj: &'a Object,
    prefix: &str,
    skip_id: bool,
    out: &mut Vec<(String, &'static str, &'a str)>,
) -> Result<(), WriteError> {
    for (key, value) in &obj.fields {
        check_ident("key", key)?;
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Str(_) if skip_id && key == "id" => {}
            Value::Str(s) => out.push((full, "=", s.value.as_str())),
            Value::Array(items) => {
                for item in items {
                    out.push((full.clone(), "+=", item.value.as_str()));
                }
            }
            Value::Object(child) => flatten(child, &full, false, out)?,
        }
    }
    Ok(())
}

/// One element on one line: `<tag id="x" key="v" list+="a" list+="b" />`.
pub fn write_element(el: &Element) -> Result<String, WriteError> {
    check_ident("tag", &el.tag)?;
    let mut out = format!("<{}", el.tag);
    if let Some(Value::Str(id)) = el.attrs.fields.get("id") {
        out.push_str(&format!(" id={}", quote(&id.value)?));
    }
    let mut attrs = Vec::new();
    flatten(&el.attrs, "", true, &mut attrs)?;
    for (key, op, val) in attrs {
        out.push_str(&format!(" {key}{op}{}", quote(val)?));
    }
    out.push_str(" />");
    Ok(out)
}

/// The whole document in canonical form (see module docs).
pub fn write_document(doc: &Document) -> Result<String, WriteError> {
    let extras = doc
        .sections
        .keys()
        .map(String::as_str)
        .filter(|n| !REQUIRED_SECTIONS.contains(n));
    let names: Vec<&str> = REQUIRED_SECTIONS.iter().copied().chain(extras).collect();

    let mut out = String::new();
    for (i, name) in names.iter().enumerate() {
        check_ident("section", name)?;
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("# {name}\n"));
        for el in doc.sections.get(*name).into_iter().flatten() {
            out.push_str("  ");
            out.push_str(&write_element(el)?);
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_espf;

    const SRC: &str = r#"# espforge
  <project name="demo" />
  <chip type="esp32c3" />
  <runtime type="blocking" />
  <console type="uart" />
  <options defaults="true" />

# peripherals
  <gpio pin="4" id="gpio4" />

# components
  <led id="red" gpio="$gpio4" />

# devices

# services
  <wifi id="wifi0" network2.ssid="work" network1.ssid="home" network1.password="pw" />
  <ntp id="ntp0" servers+="pool.ntp.org" servers+="time.google.com" />
"#;

    fn doc(src: &str) -> Document {
        let p = parse_espf(src);
        assert!(p.junk.is_empty(), "{:?}", p.junk);
        assert!(p.structure_errors.is_empty(), "{:?}", p.structure_errors);
        Document::from_sections(&p.sections).unwrap()
    }

    #[test]
    fn output_is_canonical() {
        let out = write_document(&doc(SRC)).unwrap();
        assert!(out.starts_with("# espforge\n  <project name=\"demo\" />\n"));
        // id first, rest sorted
        assert!(out.contains("<gpio id=\"gpio4\" pin=\"4\" />"));
        assert!(out.contains(
            "<wifi id=\"wifi0\" network1.password=\"pw\" network1.ssid=\"home\" network2.ssid=\"work\" />"
        ));
        // `+=` on every entry, order kept
        assert!(out.contains(
            "<ntp id=\"ntp0\" servers+=\"pool.ntp.org\" servers+=\"time.google.com\" />"
        ));
        // fixed section order, all five present
        let positions: Vec<usize> = REQUIRED_SECTIONS
            .iter()
            .map(|n| out.find(&format!("# {n}\n")).unwrap())
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn round_trip_preserves_the_value_tree() {
        let first = doc(SRC);
        let text = write_document(&first).unwrap();
        let second = doc(&text);
        assert_eq!(first, second);
        // and the writer is idempotent
        assert_eq!(text, write_document(&second).unwrap());
    }

    #[test]
    fn empty_sections_round_trip() {
        let src = "# espforge\n# peripherals\n# components\n# devices\n# services\n";
        let first = doc(src);
        let again = doc(&write_document(&first).unwrap());
        assert_eq!(first, again);
    }

    #[test]
    fn unrepresentable_values_are_rejected() {
        let mut d = doc(SRC);
        let el = &mut d.sections.get_mut("services").unwrap()[0];
        if let Some(Value::Str(id)) = el.attrs.fields.get_mut("id") {
            id.value = "has\"quote".into();
        }
        assert!(write_document(&d).is_err());
    }
}

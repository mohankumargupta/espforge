//! Typed value tree built from the parser's raw `(key, op, value)` attributes.
//!
//! The parser is lossless and does no structural validation. [`assemble`]
//! turns one element's attributes into an [`Object`]:
//!
//! * `a="x"`            -> `a: Str("x")`
//! * `a.b="x"`          -> `a: Object { b: Str("x") }`
//! * `a+="x" a+="y"`    -> `a: Array(["x", "y"])`  (`+=` on *every* entry)
//!
//! Every error carries the byte span of the offending attribute key, so
//! `diag::render_errors` can underline it. The raw `ParsedNode::attributes`
//! list stays untouched for callers that scan it.

use crate::context::{AttrOp, ParsedNode, TagAttribute};
use crate::diag::EspfError;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ops::Range;

/// A value plus the byte span it came from in the source.
///
/// Equality compares the value only, so two trees parsed from differently
/// formatted text (e.g. before and after `writer`) compare equal.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Range<usize>,
}

impl<T: PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl<T: Eq> Eq for Spanned<T> {}

pub type Map = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(Spanned<String>),
    Object(Object),
    /// Never empty: an array exists only because at least one `+=` was seen.
    Array(Vec<Spanned<String>>),
}

impl Value {
    /// Human name used in error messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Str(_) => "string",
            Value::Object(_) => "object",
            Value::Array(_) => "array",
        }
    }

    /// `kind()` with its indefinite article, for messages.
    pub fn describe(&self) -> &'static str {
        match self {
            Value::Str(_) => "a string",
            Value::Object(_) => "an object",
            Value::Array(_) => "an array",
        }
    }

    pub fn span(&self) -> Range<usize> {
        match self {
            Value::Str(s) => s.span.clone(),
            Value::Object(o) => o.span.clone(),
            Value::Array(items) => match (items.first(), items.last()) {
                (Some(first), Some(last)) => first.span.start..last.span.end,
                _ => 0..0,
            },
        }
    }
}

/// A set of named fields. The span is that of the first attribute key that
/// contributed to it (the element's tag for the root object).
#[derive(Debug, Clone)]
pub struct Object {
    pub fields: Map,
    pub span: Range<usize>,
}

/// Equality ignores spans (see [`Spanned`]).
impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields
    }
}
impl Eq for Object {}

fn mismatch(key: &str, expected: &str, found: &Value) -> EspfError {
    EspfError::at_span(
        found.span(),
        format!("`{key}` has the wrong type"),
        format!("expected {expected}, found {}", found.describe()),
    )
}

impl Object {
    pub fn new(span: Range<usize>) -> Self {
        Self {
            fields: Map::new(),
            span,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    /// `Ok(None)` when absent, `Err` (spanned) when present but not a string.
    pub fn get_str(&self, key: &str) -> Result<Option<&Spanned<String>>, EspfError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s)),
            Some(other) => Err(mismatch(key, "a string", other)),
        }
    }

    pub fn get_array(&self, key: &str) -> Result<Option<&[Spanned<String>]>, EspfError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => Ok(Some(items.as_slice())),
            Some(other) => Err(mismatch(key, "an array", other)),
        }
    }

    pub fn get_object(&self, key: &str) -> Result<Option<&Object>, EspfError> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(Value::Object(o)) => Ok(Some(o)),
            Some(other) => Err(mismatch(key, "an object", other)),
        }
    }

    /// `"true"` / `"false"` only.
    pub fn get_bool(&self, key: &str) -> Result<Option<Spanned<bool>>, EspfError> {
        let Some(s) = self.get_str(key)? else {
            return Ok(None);
        };
        match s.value.as_str() {
            "true" => Ok(Some(Spanned {
                value: true,
                span: s.span.clone(),
            })),
            "false" => Ok(Some(Spanned {
                value: false,
                span: s.span.clone(),
            })),
            other => Err(EspfError::at_span(
                s.span.clone(),
                format!("`{key}` is not a boolean"),
                format!("\"{other}\" must be \"true\" or \"false\""),
            )),
        }
    }

    pub fn get_u32(&self, key: &str) -> Result<Option<Spanned<u32>>, EspfError> {
        let Some(s) = self.get_str(key)? else {
            return Ok(None);
        };
        match s.value.parse::<u32>() {
            Ok(v) => Ok(Some(Spanned {
                value: v,
                span: s.span.clone(),
            })),
            Err(_) => Err(EspfError::at_span(
                s.span.clone(),
                format!("`{key}` is not a number"),
                format!(
                    "\"{}\" must be an unsigned integer (0..=4294967295)",
                    s.value
                ),
            )),
        }
    }

    /// Like [`get_str`](Self::get_str) but a missing key is an error located
    /// at `anchor` (typically the element's tag span).
    pub fn require_str(
        &self,
        key: &str,
        anchor: &Range<usize>,
    ) -> Result<&Spanned<String>, EspfError> {
        self.get_str(key)?.ok_or_else(|| {
            EspfError::at_span(
                anchor.clone(),
                format!("missing required attribute `{key}`"),
                format!("add {key}=\"...\" to this element"),
            )
        })
    }
}

impl ParsedNode<'_> {
    /// Assemble this element's attributes into an [`Object`].
    pub fn to_object(&self) -> Result<Object, Vec<EspfError>> {
        assemble(self)
    }
}

/// Build the value tree for one element, collecting *every* structural error.
///
/// Rules:
/// * `key+="x"` must be used on every entry of an array, including the
///   first; mixing `key="x"` and `key+="y"` at one path is an error at the
///   offending attribute.
/// * A scalar and an object at the same path conflict (`a="1"` with `a.b="2"`).
/// * Duplicate scalar keys are errors; duplicate array entries are legal.
pub fn assemble(node: &ParsedNode) -> Result<Object, Vec<EspfError>> {
    let mut errors = Vec::new();
    let mut root = Object::new(node.offset..node.offset + 1 + node.tag_name.len());
    for attr in &node.attributes {
        if let Err(e) = insert(&mut root, attr) {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(root)
    } else {
        Err(errors)
    }
}

fn insert(root: &mut Object, attr: &TagAttribute) -> Result<(), EspfError> {
    let key_span = attr.key_offset..attr.key_offset + attr.key.len();
    let val = Spanned {
        value: attr.val.to_string(),
        span: attr.val_offset..attr.val_offset + attr.val.len(),
    };
    let segs: Vec<&str> = attr.key.split('.').collect();
    insert_at(root, &segs, attr, &key_span, val)
}

fn insert_at(
    obj: &mut Object,
    segs: &[&str],
    attr: &TagAttribute,
    key_span: &Range<usize>,
    val: Spanned<String>,
) -> Result<(), EspfError> {
    let (head, rest) = segs.split_first().expect("a key has at least one segment");
    if rest.is_empty() {
        return insert_leaf(obj, head, attr, key_span, val);
    }
    let entry = obj
        .fields
        .entry((*head).to_string())
        .or_insert_with(|| Value::Object(Object::new(key_span.clone())));
    match entry {
        Value::Object(child) => insert_at(child, rest, attr, key_span, val),
        other => Err(EspfError::at_span(
            key_span.clone(),
            format!("`{head}` is both a {} and an object", other.kind()),
            format!(
                "`{head}` was already set as a {}, so `{}` cannot treat it as an object",
                other.kind(),
                attr.key
            ),
        )),
    }
}

fn insert_leaf(
    obj: &mut Object,
    leaf: &str,
    attr: &TagAttribute,
    key_span: &Range<usize>,
    val: Spanned<String>,
) -> Result<(), EspfError> {
    let err =
        |title: String, label: String| Err(EspfError::at_span(key_span.clone(), title, label));
    match obj.fields.entry(leaf.to_string()) {
        Entry::Vacant(v) => {
            v.insert(match attr.op {
                AttrOp::Set => Value::Str(val),
                AttrOp::Append => Value::Array(vec![val]),
            });
            Ok(())
        }
        Entry::Occupied(mut o) => match (o.get_mut(), attr.op) {
            (Value::Array(items), AttrOp::Append) => {
                items.push(val);
                Ok(())
            }
            (Value::Str(_), AttrOp::Set) => err(
                format!("duplicate key `{}`", attr.key),
                format!(
                    "`{}` is already set on this element; a scalar key may appear once (use `+=` for lists)",
                    attr.key
                ),
            ),
            (Value::Str(_), AttrOp::Append) => err(
                format!("`{}` mixes `=` and `+=`", attr.key),
                format!(
                    "`{0}` was set with `=` earlier; a list needs `{0}+=` on every entry, including the first",
                    attr.key
                ),
            ),
            (Value::Array(_), AttrOp::Set) => err(
                format!("`{}` mixes `+=` and `=`", attr.key),
                format!(
                    "`{0}` is a list built with `+=`; use `{0}+=` here too",
                    attr.key
                ),
            ),
            (Value::Object(_), _) => err(
                format!("`{}` is both an object and a value", attr.key),
                format!(
                    "`{0}` already has sub-keys (`{0}.…`), so it cannot also be assigned a value",
                    attr.key
                ),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_espf_runtime;

    fn assemble_first(src: &str) -> Result<Object, Vec<EspfError>> {
        let sections = parse_espf_runtime(src);
        assemble(&sections["services"][0])
    }

    fn s(v: &str) -> Spanned<String> {
        Spanned {
            value: v.into(),
            span: 0..0,
        }
    }

    #[test]
    fn scalars_objects_and_arrays_assemble() {
        let src = "# services\n  <wifi id=\"w\" network1.ssid=\"home\" network1.password=\"pw\" servers+=\"a\" servers+=\"b\" />\n";
        let o = assemble_first(src).unwrap();
        assert_eq!(o.get_str("id").unwrap().unwrap().value, "w");
        let n1 = o.get_object("network1").unwrap().unwrap();
        assert_eq!(n1.get_str("ssid").unwrap().unwrap().value, "home");
        assert_eq!(n1.get_str("password").unwrap().unwrap().value, "pw");
        assert_eq!(o.get_array("servers").unwrap().unwrap(), &[s("a"), s("b")]);
    }

    #[test]
    fn deep_dotted_keys_share_intermediate_objects() {
        let src = "# services\n  <t a.b.c=\"1\" a.b.d=\"2\" a.e=\"3\" />\n";
        let o = assemble_first(src).unwrap();
        let ab = o
            .get_object("a")
            .unwrap()
            .unwrap()
            .get_object("b")
            .unwrap()
            .unwrap();
        assert_eq!(ab.get_str("c").unwrap().unwrap().value, "1");
        assert_eq!(ab.get_str("d").unwrap().unwrap().value, "2");
        assert_eq!(
            o.get_object("a")
                .unwrap()
                .unwrap()
                .get_str("e")
                .unwrap()
                .unwrap()
                .value,
            "3"
        );
    }

    #[test]
    fn value_spans_point_into_the_source() {
        let src = "# services\n  <t a.b=\"hello\" />\n";
        let o = assemble_first(src).unwrap();
        let v = o
            .get_object("a")
            .unwrap()
            .unwrap()
            .get_str("b")
            .unwrap()
            .unwrap();
        assert_eq!(&src[v.span.clone()], "hello");
    }

    #[test]
    fn duplicate_array_entries_are_legal() {
        let src = "# services\n  <t s+=\"x\" s+=\"x\" />\n";
        let o = assemble_first(src).unwrap();
        assert_eq!(o.get_array("s").unwrap().unwrap().len(), 2);
    }

    #[test]
    fn duplicate_scalar_is_an_error_at_the_second_key() {
        let src = "# services\n  <t a=\"1\" a=\"2\" />\n";
        let errs = assemble_first(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].title.contains("duplicate key"));
        assert_eq!(&src[errs[0].span.clone()], "a");
        assert_eq!(errs[0].span.start, src.rfind("a=").unwrap());
    }

    #[test]
    fn set_then_append_is_an_error_at_the_append() {
        let src = "# services\n  <t a=\"1\" a+=\"2\" />\n";
        let errs = assemble_first(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].title.contains("mixes"));
        assert_eq!(errs[0].span.start, src.find("a+=").unwrap());
        assert_eq!(&src[errs[0].span.clone()], "a");
    }

    #[test]
    fn append_then_set_is_an_error_at_the_set() {
        let src = "# services\n  <t a+=\"1\" a=\"2\" />\n";
        let errs = assemble_first(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].title.contains("mixes"));
        assert_eq!(errs[0].span.start, src.rfind("a=").unwrap());
    }

    #[test]
    fn scalar_then_object_conflicts_at_the_dotted_key() {
        let src = "# services\n  <t a=\"1\" a.b=\"2\" />\n";
        let errs = assemble_first(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(&src[errs[0].span.clone()], "a.b");
    }

    #[test]
    fn object_then_scalar_conflicts_at_the_scalar_key() {
        let src = "# services\n  <t a.b=\"2\" a=\"1\" />\n";
        let errs = assemble_first(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].span.start, src.rfind("a=").unwrap());
        assert_eq!(&src[errs[0].span.clone()], "a");
    }

    #[test]
    fn array_then_object_conflicts() {
        let src = "# services\n  <t a+=\"1\" a.b=\"2\" />\n";
        let errs = assemble_first(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(&src[errs[0].span.clone()], "a.b");
    }

    #[test]
    fn every_error_is_reported_not_just_the_first() {
        let src = "# services\n  <t a=\"1\" a=\"2\" b+=\"1\" b=\"2\" />\n";
        assert_eq!(assemble_first(src).unwrap_err().len(), 2);
    }

    #[test]
    fn accessors_report_type_mismatches_at_the_value() {
        let src =
            "# services\n  <t a.b=\"1\" l+=\"x\" n=\"12\" t=\"true\" bad=\"maybe\" big=\"-1\" />\n";
        let o = assemble_first(src).unwrap();
        // object / array used as a string
        let e = o.get_str("a").unwrap_err();
        assert!(e.title.contains("wrong type"));
        assert!(o.get_str("l").is_err());
        // string used as object / array
        assert!(o.get_object("n").is_err());
        assert!(o.get_array("n").is_err());
        // typed scalars
        assert_eq!(o.get_u32("n").unwrap().unwrap().value, 12);
        assert!(o.get_bool("t").unwrap().unwrap().value);
        let e = o.get_bool("bad").unwrap_err();
        assert_eq!(&src[e.span.clone()], "maybe");
        let e = o.get_u32("big").unwrap_err();
        assert_eq!(&src[e.span.clone()], "-1");
        // absent keys are Ok(None)
        assert!(o.get_str("nope").unwrap().is_none());
    }

    #[test]
    fn require_str_reports_missing_key_at_the_anchor() {
        let src = "# services\n  <t a=\"1\" />\n";
        let o = assemble_first(src).unwrap();
        let e = o.require_str("name", &o.span).unwrap_err();
        assert_eq!(&src[e.span.clone()], "<t");
        assert!(e.title.contains("name"));
    }

    #[test]
    fn raw_attributes_stay_available() {
        let src = "# services\n  <t a+=\"1\" a+=\"2\" />\n";
        let sections = parse_espf_runtime(src);
        let node = &sections["services"][0];
        let _ = node.to_object().unwrap();
        assert_eq!(node.attributes.len(), 2);
    }
}

//! The coercion layer for schema-generated element attributes.
//!
//! `generated_elements.rs` (written by `build.rs`) declares one `Attrs` struct
//! per element in `espf.schema.kdl`: one field per declared attribute, typed by
//! that attribute's `kind`, carrying the byte span it came from. The generated
//! structs call the helpers below, which are the only place a spelling
//! (`"0x48"`, `"true"`, `"$gpio18"`, `"output"`) becomes a typed value.
//!
//! Two rules keep this layer honest:
//!
//! - **The schema owns presence and kind.** "Is `address` required", "is `mode`
//!   0..=3", "is `parity` one of none/even/odd" are read from
//!   `espf.schema.kdl`, never restated in lowering.
//! - **The message is not owned here.** A coercion returns an [`Issue`]; the
//!   element's lowering turns it into the text the user sees. That keeps this
//!   step byte-identical for existing errors, and leaves one place per element
//!   to change when diagnostics become codes (extreme.md §3.4).

use espf::context::{ParsedNode, TagAttribute};

/// A byte range in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte of the value, as an offset into the `.espf` file.
    pub start: usize,
    /// One past the last byte of the value.
    pub end: usize,
}

impl Span {
    /// A zero-width span at the element's tag: where a defaulted attribute
    /// "comes from".
    pub fn at_node(node: &ParsedNode<'_>) -> Self {
        Self {
            start: node.offset,
            end: node.offset,
        }
    }
}

/// A coerced attribute value together with the span it was written at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

/// A `$name` reference to another element, as in `gpio="$gpio18"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ref<'a> {
    /// The value as written, including the `$` when the author used one.
    pub raw: &'a str,
    /// The referenced id, without the `$`.
    pub name: &'a str,
    pub span: Span,
}

impl<'a> Ref<'a> {
    fn at(raw: &'a str, span: Span) -> Self {
        Self {
            raw,
            name: raw.trim_start_matches('$'),
            span,
        }
    }
}

/// A chip-select (`kind="pin_or_ref"`): a GPIO number or a `<gpio>` id.
///
/// Both spellings are a pin in the end; which one the author wrote is kept
/// because turning a `<gpio>` id into a number needs the gpio table, which is
/// the element's business, not this layer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinOrRef<'a> {
    /// `cs="9"`: the GPIO number itself.
    Pin(Spanned<u8>),
    /// `cs="gpio5"` or `cs="$gpio5"`: a `<gpio>` id.
    Ref(Ref<'a>),
}

/// Why an attribute could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issue<'a> {
    /// Not written, or written empty, where the schema says required.
    Missing { attr: &'static str },
    /// Written, but not a value of the declared kind.
    Invalid {
        attr: &'static str,
        /// The value as written, so the message can quote it.
        value: &'a str,
    },
}

fn find<'a>(node: &'a ParsedNode<'a>, attr: &str) -> Option<&'a TagAttribute<'a>> {
    node.attributes.iter().find(|a| a.key == attr)
}

fn span_of(attr: &TagAttribute<'_>) -> Span {
    Span {
        start: attr.val_offset,
        end: attr.val_offset + attr.val.len(),
    }
}

fn missing<'a>(attr: &'static str) -> Issue<'a> {
    Issue::Missing { attr }
}

fn invalid<'a>(attr: &'static str, value: &'a str) -> Issue<'a> {
    Issue::Invalid { attr, value }
}

fn shown<'a>(attr: &'a TagAttribute<'a>) -> Spanned<&'a str> {
    Spanned {
        value: attr.val,
        span: span_of(attr),
    }
}

/// A required identifier: written and non-empty.
pub fn ident<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Spanned<&'a str>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    if attr.val.is_empty() {
        return Err(missing(name));
    }
    Ok(shown(attr))
}

/// A required free text value.
pub fn text<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Spanned<&'a str>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    Ok(shown(attr))
}

/// A free text value, defaulted from the schema when absent.
pub fn text_or<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    default: &'a str,
) -> Result<Spanned<&'a str>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => Ok(shown(attr)),
        None => Ok(Spanned {
            value: default,
            span: Span::at_node(node),
        }),
    }
}

/// An optional free text value.
pub fn opt_text<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Option<Spanned<&'a str>>, Issue<'a>> {
    Ok(find(node, name).map(shown))
}

/// `kind="freq"`: kept as the text the author wrote.
///
/// The units and their limits ("400kHz", "40MHz", and the peripheral-specific
/// "not supported yet") belong to the peripheral that applies the frequency,
/// where each already has its own wording. Phase 2 moves the parse and that
/// wording together; until then a second parser here would be a third
/// vocabulary.
pub fn frequency<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Spanned<&'a str>, Issue<'a>> {
    text(node, name)
}

/// A `kind="freq"` value with a schema default.
pub fn frequency_or<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    default: &'a str,
) -> Result<Spanned<&'a str>, Issue<'a>> {
    text_or(node, name, default)
}

/// An optional `kind="freq"` value.
pub fn opt_frequency<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Option<Spanned<&'a str>>, Issue<'a>> {
    opt_text(node, name)
}

/// A required `true` / `false`.
pub fn flag<'a>(node: &'a ParsedNode<'a>, name: &'static str) -> Result<Spanned<bool>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    flag_value(attr, name)
}

/// A `true` / `false` value, defaulted from the schema when absent.
pub fn flag_or<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    default: bool,
) -> Result<Spanned<bool>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => flag_value(attr, name),
        None => Ok(Spanned {
            value: default,
            span: Span::at_node(node),
        }),
    }
}

fn flag_value<'a>(
    attr: &'a TagAttribute<'a>,
    name: &'static str,
) -> Result<Spanned<bool>, Issue<'a>> {
    let value = match attr.val {
        "true" => true,
        "false" => false,
        other => return Err(invalid(name, other)),
    };
    Ok(Spanned {
        value,
        span: span_of(attr),
    })
}

/// A required `u32`.
pub fn number<'a>(node: &'a ParsedNode<'a>, name: &'static str) -> Result<Spanned<u32>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    number_value(attr, name)
}

/// A required `u32` with a schema `range`.
pub fn number_in<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    low: u32,
    high: u32,
) -> Result<Spanned<u32>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    let value = attr
        .val
        .parse::<u32>()
        .map_err(|_| invalid(name, attr.val))?;
    if !(low..=high).contains(&value) {
        return Err(invalid(name, attr.val));
    }
    Ok(Spanned {
        value,
        span: span_of(attr),
    })
}

/// A `u32` value, defaulted from the schema when absent.
pub fn number_or<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    default: u32,
) -> Result<Spanned<u32>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => number_value(attr, name),
        None => Ok(Spanned {
            value: default,
            span: Span::at_node(node),
        }),
    }
}

/// A `u32` with a schema `range`, defaulted when absent.
pub fn number_in_or<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    low: u32,
    high: u32,
    default: u32,
) -> Result<Spanned<u32>, Issue<'a>> {
    let Some(attr) = find(node, name) else {
        return Ok(Spanned {
            value: default,
            span: Span::at_node(node),
        });
    };
    let value = attr
        .val
        .parse::<u32>()
        .map_err(|_| invalid(name, attr.val))?;
    if !(low..=high).contains(&value) {
        return Err(invalid(name, attr.val));
    }
    Ok(Spanned {
        value,
        span: span_of(attr),
    })
}

fn number_value<'a>(
    attr: &'a TagAttribute<'a>,
    name: &'static str,
) -> Result<Spanned<u32>, Issue<'a>> {
    let value = attr
        .val
        .parse::<u32>()
        .map_err(|_| invalid(name, attr.val))?;
    Ok(Spanned {
        value,
        span: span_of(attr),
    })
}

/// A required GPIO number.
pub fn pin<'a>(node: &'a ParsedNode<'a>, name: &'static str) -> Result<Spanned<u8>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    Ok(Spanned {
        value: attr
            .val
            .parse::<u8>()
            .map_err(|_| invalid(name, attr.val))?,
        span: span_of(attr),
    })
}

/// An optional GPIO number.
pub fn opt_pin<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Option<Spanned<u8>>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => Ok(Some(Spanned {
            value: attr
                .val
                .parse::<u8>()
                .map_err(|_| invalid(name, attr.val))?,
            span: span_of(attr),
        })),
        None => Ok(None),
    }
}

/// A required `0x..` or decimal byte.
pub fn hex<'a>(node: &'a ParsedNode<'a>, name: &'static str) -> Result<Spanned<u8>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    hex_value(attr, name)
}

/// An optional `0x..` or decimal byte.
pub fn opt_hex<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Option<Spanned<u8>>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => Ok(Some(hex_value(attr, name)?)),
        None => Ok(None),
    }
}

fn hex_value<'a>(attr: &'a TagAttribute<'a>, name: &'static str) -> Result<Spanned<u8>, Issue<'a>> {
    let parsed = match attr
        .val
        .strip_prefix("0x")
        .or_else(|| attr.val.strip_prefix("0X"))
    {
        Some(hex) => u8::from_str_radix(hex, 16),
        None => attr.val.parse::<u8>(),
    };
    Ok(Spanned {
        value: parsed.map_err(|_| invalid(name, attr.val))?,
        span: span_of(attr),
    })
}

/// A required `kind="pin_or_ref"` value: a GPIO number, or anything else as
/// a `<gpio>` reference.
pub fn pin_or_ref<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<PinOrRef<'a>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    let span = span_of(attr);
    Ok(match attr.val.parse::<u8>() {
        Ok(pin) => PinOrRef::Pin(Spanned { value: pin, span }),
        Err(_) => PinOrRef::Ref(Ref::at(attr.val, span)),
    })
}

/// A required `$`-reference to another element.
pub fn reference<'a>(node: &'a ParsedNode<'a>, name: &'static str) -> Result<Ref<'a>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    Ok(Ref::at(attr.val, span_of(attr)))
}

/// An optional `$`-reference to another element.
pub fn opt_reference<'a>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
) -> Result<Option<Ref<'a>>, Issue<'a>> {
    Ok(find(node, name).map(|attr| Ref::at(attr.val, span_of(attr))))
}

/// A required value from a generated `choices` enum.
///
/// `parse` is the enum's own [`parse`](crate::components::generated_elements)
/// function, which is generated from the same `choices` list this checks
/// against — so the enum and the validation cannot disagree.
pub fn parsed<'a, T: Copy>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    parse: fn(&str) -> Option<T>,
) -> Result<Spanned<T>, Issue<'a>> {
    let attr = find(node, name).ok_or(missing(name))?;
    parsed_value(attr, name, parse)
}

/// A `choices` value, defaulted from the schema when absent.
pub fn parsed_or<'a, T: Copy>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    parse: fn(&str) -> Option<T>,
    default: T,
) -> Result<Spanned<T>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => parsed_value(attr, name, parse),
        None => Ok(Spanned {
            value: default,
            span: Span::at_node(node),
        }),
    }
}

/// An optional `choices` value.
pub fn opt_parsed<'a, T: Copy>(
    node: &'a ParsedNode<'a>,
    name: &'static str,
    parse: fn(&str) -> Option<T>,
) -> Result<Option<Spanned<T>>, Issue<'a>> {
    match find(node, name) {
        Some(attr) => Ok(Some(parsed_value(attr, name, parse)?)),
        None => Ok(None),
    }
}

fn parsed_value<'a, T: Copy>(
    attr: &'a TagAttribute<'a>,
    name: &'static str,
    parse: fn(&str) -> Option<T>,
) -> Result<Spanned<T>, Issue<'a>> {
    Ok(Spanned {
        value: parse(attr.val).ok_or_else(|| invalid(name, attr.val))?,
        span: span_of(attr),
    })
}

/// A `kind="list"` attribute: every occurrence, in source order.
///
/// An absent list is empty, so this cannot fail. The `+` spelling
/// (`servers+="pool.ntp.org"`) and the plain one are both collected.
pub fn list<'a>(node: &'a ParsedNode<'a>, name: &str) -> Vec<Spanned<&'a str>> {
    node.attributes
        .iter()
        .filter(|a| a.key == name)
        .map(shown)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node<'a>(section: &str, src: &'a str) -> ParsedNode<'a> {
        espf::parser::parse_espf(src).sections[section][0].clone()
    }

    #[test]
    fn a_reference_keeps_both_spellings_and_its_span() {
        let n = node(
            "components",
            "# components\n  <led id=\"l\" gpio=\"$gpio18\" />\n",
        );
        let r = reference(&n, "gpio").unwrap();
        assert_eq!(r.raw, "$gpio18");
        assert_eq!(r.name, "gpio18");
        let written_at = &"# components\n  <led id=\"l\" gpio="[..];
        assert_eq!(r.span.start, written_at.len() + 1);
        assert_eq!(r.span.end - r.span.start, "$gpio18".len());
    }

    #[test]
    fn missing_identifiers_are_missing_even_when_empty() {
        let n = node(
            "components",
            "# components\n  <led id=\"\" gpio=\"$gpio18\" />\n",
        );
        assert_eq!(ident(&n, "id"), Err(Issue::Missing { attr: "id" }));
    }

    #[test]
    fn a_bad_choice_reports_the_value_as_written() {
        let n = node(
            "components",
            "# components\n  <led id=\"l\" gpio=\"$g\" active_low=\"yes\" />\n",
        );
        assert_eq!(
            flag(&n, "active_low"),
            Err(Issue::Invalid {
                attr: "active_low",
                value: "yes"
            })
        );
    }

    #[test]
    fn hex_accepts_both_spellings_and_a_range_is_enforced() {
        let hi = node(
            "components",
            "# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n",
        );
        assert_eq!(hex(&hi, "address").unwrap().value, 0x48);

        let range = node(
            "peripherals",
            "# peripherals\n  <spi id=\"s\" sclk=\"1\" mosi=\"2\" mode=\"3\" />\n",
        );
        assert_eq!(number_in_or(&range, "mode", 0, 3, 0).unwrap().value, 3);
        assert_eq!(
            number_in_or(&range, "mode", 0, 2, 0),
            Err(Issue::Invalid {
                attr: "mode",
                value: "3"
            })
        );
    }

    #[test]
    fn defaults_are_attributed_to_the_element() {
        let n = node("espforge", "# espforge\n  <console />\n");
        let default = text_or(&n, "type", "uart").unwrap();
        assert_eq!(default.value, "uart");
        assert_eq!(default.span, Span::at_node(&n));
    }

    #[test]
    fn a_list_collects_every_occurrence() {
        let n = node(
            "services",
            "# services\n  <ntp id=\"ntp0\" servers+=\"pool.ntp.org\" servers+=\"time.google.com\" />\n",
        );
        let servers: Vec<&str> = list(&n, "servers").into_iter().map(|s| s.value).collect();
        assert_eq!(servers, ["pool.ntp.org", "time.google.com"]);
    }
}

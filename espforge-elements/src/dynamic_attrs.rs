//! Attribute validation for elements that only exist at runtime.
//!
//! `generated::validate` is the fast path for every bundled tag: `build.rs`
//! wrote one typed reader per element, and the readers own presence, kind,
//! choices and defaults. A project manifest's tag was not in the schema when
//! the build script ran, so it has no reader. This module is the same checks
//! read from [`espforge_schema::Element`] instead of from generated code, and
//! it produces the same [`Issue`], so the caller's wording is unchanged.
//!
//! Defaults are deliberately not applied here: a defaulted value is filled in
//! by the lowering that reads it (`spelling` in `devices::recipe`), exactly as
//! the generated readers return the default rather than failing.

use espf::context::ParsedNode;
use espforge_schema::{AttrKind, Element};

use crate::attrs::Issue;

/// Validate `node` against the attributes `element` declares.
///
/// `Missing` is a required attribute that is absent (or, for an identifier,
/// empty); `Invalid` is a value that is not of the declared kind, outside the
/// declared `range`, or not one of the declared `choices`.
pub fn validate<'a>(element: &'a Element, node: &'a ParsedNode<'a>) -> Result<(), Issue<'a>> {
    for attr in &element.attrs {
        // The name borrows from the schema this run lowers against, which is
        // why `Issue` carries a lifetime rather than a `&'static str`.
        let name = attr.name.as_str();
        let Some(value) = node.get_attr(&attr.name) else {
            if attr.required {
                return Err(Issue::Missing { attr: name });
            }
            continue;
        };
        if attr.required && matches!(attr.kind, AttrKind::Ident) && value.is_empty() {
            return Err(Issue::Missing { attr: name });
        }
        check(name, attr, value)?;
    }
    Ok(())
}

fn check<'a>(
    name: &'a str,
    attr: &espforge_schema::AttrDef,
    value: &'a str,
) -> Result<(), Issue<'a>> {
    let invalid = || Issue::Invalid { attr: name, value };
    match &attr.kind {
        // Free text, an identifier and a reference are accepted as written;
        // the reference target is checked where it is resolved.
        AttrKind::Str | AttrKind::Freq | AttrKind::Ref { .. } | AttrKind::PinOrRef => {}
        AttrKind::Ident => {
            if value.is_empty() {
                return Err(invalid());
            }
        }
        AttrKind::Pin => {
            value.parse::<u8>().map_err(|_| invalid())?;
        }
        AttrKind::Hex => {
            hex_byte(value).ok_or_else(invalid)?;
        }
        AttrKind::U32 => {
            let number: u32 = value.parse().map_err(|_| invalid())?;
            if let Some((low, high)) = attr.range {
                if !(low..=high).contains(&number) {
                    return Err(invalid());
                }
            }
        }
        AttrKind::Bool => {
            if !matches!(value, "true" | "false") {
                return Err(invalid());
            }
        }
        AttrKind::Choice(choices) => {
            if !choices.iter().any(|choice| choice == value) {
                return Err(invalid());
            }
        }
        // A `list` attribute is repeated with `+=`; nothing in the bundled
        // schema lowers one yet, and the generated readers refuse it at build
        // time, so a runtime element cannot use it either.
        AttrKind::List(_) => {
            return Err(invalid());
        }
    }
    Ok(())
}

/// A `0x..` or decimal byte, as `attrs::hex` reads it.
fn hex_byte(text: &str) -> Option<u8> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element() -> &'static Element {
        static SCHEMA: std::sync::OnceLock<espforge_schema::Schema> = std::sync::OnceLock::new();
        SCHEMA
            .get_or_init(|| espforge_schema::load_all(&[espforge_schema::SCHEMA_KDL]).unwrap())
            .element("gpio")
            .expect("`<gpio>` is a core element")
    }

    fn node<'a>(src: &'a str) -> ParsedNode<'a> {
        espf::parser::parse_espf(src).sections["peripherals"][0].clone()
    }

    #[test]
    fn a_valid_element_passes() {
        let n = node("# peripherals\n  <gpio id=\"g\" pin=\"18\" direction=\"output\" />\n");
        assert_eq!(validate(element(), &n), Ok(()));
    }

    #[test]
    fn a_missing_required_attribute_is_missing() {
        let n = node("# peripherals\n  <gpio id=\"g\" direction=\"output\" />\n");
        assert_eq!(validate(element(), &n), Err(Issue::Missing { attr: "pin" }));
    }

    #[test]
    fn an_invalid_choice_reports_the_value() {
        let n = node("# peripherals\n  <gpio id=\"g\" pin=\"18\" direction=\"sideways\" />\n");
        assert_eq!(
            validate(element(), &n),
            Err(Issue::Invalid {
                attr: "direction",
                value: "sideways"
            })
        );
    }

    #[test]
    fn a_pin_outside_the_byte_range_is_invalid() {
        let n = node("# peripherals\n  <gpio id=\"g\" pin=\"999\" />\n");
        assert_eq!(
            validate(element(), &n),
            Err(Issue::Invalid {
                attr: "pin",
                value: "999"
            })
        );
    }
}

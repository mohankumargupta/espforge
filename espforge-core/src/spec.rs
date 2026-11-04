//! Editing a spec's own `<chip>`: what `espforge new` writes.
//!
//! A scaffold starts from an example's text, so overriding the chip has to
//! change one attribute value and leave every other byte — banner, comments,
//! indentation — as it was. The parser reports the value's byte offset, so the
//! edit is an exact splice rather than a search for `type="`.

use espf::parser::parse_espf;

/// The chip the spec's `<chip type="..." />` names.
pub fn spec_chip(spec: &str) -> Result<&str, String> {
    Ok(chip_value(spec)?.1)
}

/// The same spec with `<chip type="..." />` set to `chip`.
///
/// Everything outside the attribute's value is preserved byte for byte, which
/// is why this is a splice and not a rewrite.
pub fn spec_with_chip(spec: &str, chip: &str) -> Result<String, String> {
    let (offset, current) = chip_value(spec)?;
    let mut out = String::with_capacity(spec.len() + chip.len());
    out.push_str(&spec[..offset]);
    out.push_str(chip);
    out.push_str(&spec[offset + current.len()..]);
    Ok(out)
}

/// `(byte offset, value)` of the single `<chip>` node's `type` attribute.
fn chip_value(spec: &str) -> Result<(usize, &str), String> {
    let parsed = parse_espf(spec);
    let chips: Vec<_> = parsed
        .sections
        .values()
        .flatten()
        .filter(|node| node.tag_name == "chip")
        .collect();

    let node = match chips.as_slice() {
        [] => return Err("the spec declares no <chip type=\"...\" />".to_string()),
        [node] => *node,
        _ => return Err("the spec declares more than one <chip>".to_string()),
    };

    node.get_attr_spanned("type")
        .map(|attr| (attr.val_offset, attr.val))
        .ok_or_else(|| "<chip> has no type attribute".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blink-shaped spec with everything a splice must not disturb.
    const SPEC: &str = "\
====================
BLINK EXAMPLE
====================

# espforge
  <project name=\"blink\" />
  <chip type=\"esp32c3\" />
  <runtime type=\"blocking\" />

# peripherals
  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />
";

    #[test]
    fn the_chip_is_read_from_the_spec() {
        assert_eq!(spec_chip(SPEC).unwrap(), "esp32c3");
    }

    #[test]
    fn rewriting_the_chip_touches_nothing_else() {
        let out = spec_with_chip(SPEC, "esp32s3").unwrap();
        assert!(out.contains("<chip type=\"esp32s3\" />"), "{out}");
        // Only the value changed: the rest of the file is byte-identical.
        assert_eq!(out, SPEC.replace("esp32c3", "esp32s3"));
        assert_eq!(SPEC.len(), out.len());
    }

    #[test]
    fn rewriting_to_the_same_chip_is_the_same_text() {
        assert_eq!(spec_with_chip(SPEC, "esp32c3").unwrap(), SPEC);
    }

    #[test]
    fn attributes_around_the_type_survive() {
        let spec = "# espforge\n  <chip other=\"1\" type=\"esp32c3\" name=\"x\" />\n";
        let out = spec_with_chip(spec, "esp32c6").unwrap();
        assert_eq!(
            out,
            "# espforge\n  <chip other=\"1\" type=\"esp32c6\" name=\"x\" />\n"
        );
    }

    #[test]
    fn a_spec_without_a_chip_is_an_error() {
        let err = spec_with_chip("# espforge\n  <project name=\"p\" />\n", "esp32c3").unwrap_err();
        assert!(err.contains("no <chip"), "{err}");
    }

    #[test]
    fn a_chip_without_a_type_is_an_error() {
        let err = spec_with_chip("# espforge\n  <chip />\n", "esp32c3").unwrap_err();
        assert!(err.contains("no type attribute"), "{err}");
    }

    #[test]
    fn two_chips_are_an_error_not_a_guess() {
        let spec = "# espforge\n  <chip type=\"esp32c3\" />\n  <chip type=\"esp32s3\" />\n";
        let err = spec_with_chip(spec, "esp32c6").unwrap_err();
        assert!(err.contains("more than one <chip>"), "{err}");
    }
}

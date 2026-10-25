//! Parsed syntax and the `$` reference graph.
//!
//! [`ParsedNode`] is what the parser produces: a tag name, its id, attributes
//! with byte spans, and the node's offset. [`Refs`] is the graph lowering needs
//! — which ids exist and what kind of peripheral each one names — with owned
//! strings instead of handle tables.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `key="v"`
    Set,
    /// `key+="v"`
    Append,
}

#[derive(Debug, Clone)]
pub struct TagAttribute<'a> {
    /// Full key, possibly dotted (`network1.ssid`).
    pub key: &'a str,
    pub op: AttrOp,
    /// Byte offset of the key's first character in the original file.
    pub key_offset: usize,
    pub val: &'a str,
    /// Byte offset of the value's first character in the original file.
    pub val_offset: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedNode<'a> {
    pub tag_name: &'a str,
    pub id: &'a str,
    pub attributes: Vec<TagAttribute<'a>>,
    /// Byte offset of the node's tag start in the original file.
    pub offset: usize,
}

impl<'a> ParsedNode<'a> {
    pub fn get_attr(&self, key: &str) -> Option<&'a str> {
        self.attributes.iter().find(|a| a.key == key).map(|a| a.val)
    }

    pub fn get_attr_spanned(&self, key: &str) -> Option<&TagAttribute<'a>> {
        self.attributes.iter().find(|a| a.key == key)
    }
}

/// What kind of hardware a `$` reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeripheralKind {
    Gpio,
    I2c,
    Spi,
    Uart,
}

/// The `$` graph: the ids every section declares and the peripheral kind each
/// peripheral id names. The ids are owned, so the result can live next to the
/// parsed file it describes and be built more than once without borrowing.
#[derive(Debug, Clone, Default)]
pub struct Refs {
    ids: HashSet<String>,
    peripherals: HashMap<String, PeripheralKind>,
}

impl Refs {
    /// Resolve every `$` reference and report the graph problems in source
    /// order: missing ids, a component without a controller link, a reference
    /// to an id that is not defined yet, and malformed i2c pins.
    pub fn resolve(
        sections: &HashMap<String, Vec<ParsedNode>>,
    ) -> Result<Self, Vec<crate::diag::EspfError>> {
        let mut refs = Self::default();
        let mut errors = Vec::new();

        for peri in sections
            .get("peripherals")
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            if peri.id.is_empty() {
                errors.push(crate::diag::EspfError::at_node(
                    peri,
                    "peripheral is missing an id",
                    format!(
                        "<{}> needs an id=\"...\" attribute so `$` references can bind to it",
                        peri.tag_name
                    ),
                ));
                continue;
            }
            refs.ids.insert(peri.id.to_string());
            let kind = match peri.tag_name {
                "i2c" => PeripheralKind::I2c,
                "spi" => PeripheralKind::Spi,
                "uart" => PeripheralKind::Uart,
                _ => PeripheralKind::Gpio,
            };
            if kind == PeripheralKind::I2c {
                for key in ["sda", "scl"] {
                    let val = peri.get_attr(key).unwrap_or("0");
                    if val.parse::<u8>().is_err() {
                        let error = peri
                            .get_attr_spanned(key)
                            .map(|attr| {
                                crate::diag::EspfError::at_attr(
                                    attr,
                                    "invalid pin number",
                                    format!("`{key}=\"{val}\"` must be a number in 0..=255"),
                                )
                            })
                            .unwrap_or_else(|| {
                                crate::diag::EspfError::at_node(
                                    peri,
                                    "invalid pin number",
                                    format!("`{key}=\"{val}\"` must be a number in 0..=255"),
                                )
                            });
                        errors.push(error);
                    }
                }
            }
            refs.peripherals.insert(peri.id.to_string(), kind);
        }

        for comp in sections.get("components").map(Vec::as_slice).unwrap_or(&[]) {
            if comp.id.is_empty() {
                errors.push(crate::diag::EspfError::at_node(
                    comp,
                    "component is missing an id",
                    "<i2c-...> elements need an id=\"...\" attribute",
                ));
                continue;
            }
            refs.ids.insert(comp.id.to_string());

            let Some(link) = comp
                .attributes
                .iter()
                .find(|a| a.key == "i2c" || a.key == "spi" || a.key == "gpio" || a.key == "uart")
            else {
                errors.push(crate::diag::EspfError::at_node(
                    comp,
                    "component is missing its controller link",
                    format!(
                        "<{} id=\"{}\"> needs an i2c=\"/spi=\"/gpio=\"/uart=\" attribute pointing at a peripheral id",
                        comp.tag_name, comp.id
                    ),
                ));
                continue;
            };

            let parent = link.val.trim_start_matches('$');
            if !refs.ids.contains(parent) {
                errors.push(crate::diag::EspfError::at_attr(
                    link,
                    "unknown peripheral",
                    format!("there is no `{parent}` in `# peripherals`; the `$` reference must match an id defined there"),
                ));
            }
        }

        for dev in sections.get("devices").map(Vec::as_slice).unwrap_or(&[]) {
            if dev.id.is_empty() {
                errors.push(crate::diag::EspfError::at_node(
                    dev,
                    "device is missing an id",
                    format!("<{}> needs an id=\"...\" attribute", dev.tag_name),
                ));
                continue;
            }
            refs.ids.insert(dev.id.to_string());

            let Some(mapping) = dev.attributes.iter().find(|a| a.key.contains("component")) else {
                errors.push(crate::diag::EspfError::at_node(
                    dev,
                    "device is missing its component mapping",
                    format!(
                        "<{} id=\"{}\"> needs a `component` attribute pointing at an id in `# components`",
                        dev.tag_name, dev.id
                    ),
                ));
                continue;
            };

            let component = mapping.val.trim_start_matches('$');
            if !refs.ids.contains(component) {
                errors.push(crate::diag::EspfError::at_attr(
                    mapping,
                    "unknown component",
                    format!("there is no `{component}` in `# components`; the `$` reference must match an id defined there"),
                ));
            }
        }

        if errors.is_empty() {
            Ok(refs)
        } else {
            Err(errors)
        }
    }

    /// The peripheral kind `name` names, if it is a peripheral.
    pub fn kind(&self, name: &str) -> Option<PeripheralKind> {
        self.peripherals.get(name).copied()
    }

    /// Whether any element declared `name` (a peripheral, component or device).
    pub fn knows(&self, name: &str) -> bool {
        self.ids.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{EspfError, line_number_for_offset};
    use crate::parser::parse_espf_runtime;

    /// A `$`-reference pointing at an unknown id is reported as a span-aware
    /// error pointing at the offending value, not a string blob.
    #[test]
    fn unresolved_reference_points_at_value() {
        let src = "# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <oled id=\"oled1\" i2c=\"$i2cX\" address=\"0x3c\" />\n";
        let sections = parse_espf_runtime(src);
        let Err(errs) = Refs::resolve(&sections) else {
            panic!("expected resolution failure");
        };
        assert_eq!(errs.len(), 1, "one error: {errs:?}");
        let EspfError { title, span, .. } = &errs[0];
        assert_eq!(title, "unknown peripheral");
        assert_eq!(&src[span.start..span.end], "$i2cX");
        assert_eq!(line_number_for_offset(src, span.start), 4);
    }

    #[test]
    fn valid_graph_records_kinds_and_ids() {
        let src = "# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <oled id=\"oled1\" i2c=\"$i2c0\" address=\"0x3c\" />\n# devices\n  <smile id=\"smile1\" component=\"$oled1\" />\n";
        let sections = parse_espf_runtime(src);
        let refs = Refs::resolve(&sections).expect("valid graph resolves");
        assert_eq!(refs.kind("i2c0"), Some(PeripheralKind::I2c));
        assert!(refs.knows("oled1"), "component id is known");
        assert!(refs.knows("smile1"), "device id is known");
        assert_eq!(refs.kind("oled1"), None, "a component is not a peripheral");
    }

    #[test]
    fn malformed_pin_value_is_an_error_not_a_panic() {
        let src = "# peripherals\n  <i2c id=\"i2c0\" sda=\"nine\" scl=\"5\" />\n";
        let sections = parse_espf_runtime(src);
        let Err(errs) = Refs::resolve(&sections) else {
            panic!("expected pin parse failure");
        };
        let Some(bad) = errs
            .iter()
            .find(|e: &&EspfError| e.title == "invalid pin number")
        else {
            panic!("expected invalid pin number error, got {errs:?}");
        };
        assert_eq!(&src[bad.span.start..bad.span.end], "nine");
    }

    #[test]
    fn missing_link_and_missing_component_mapping_are_located() {
        let src = "# components\n  <relay id=\"r1\" />\n# devices\n  <lamp id=\"l1\" />\n";
        let sections = parse_espf_runtime(src);
        let Err(errs) = Refs::resolve(&sections) else {
            panic!("expected resolution failure");
        };
        assert_eq!(errs.len(), 2, "one per node: {errs:?}");
        // Each error underlines `<relay` / `<lamp` at its true offset.
        for e in &errs {
            assert!(
                src[e.span.start..].starts_with('<'),
                "span {e:?} not at tag"
            );
        }
    }
}

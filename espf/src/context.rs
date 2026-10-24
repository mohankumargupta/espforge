use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(pub u16);

#[derive(Default)]
pub struct IdInterner<'a> {
    map: HashMap<&'a str, Handle>,
    strings: Vec<&'a str>,
}

impl<'a> IdInterner<'a> {
    pub fn get_or_insert(&mut self, id: &'a str) -> Handle {
        if let Some(&handle) = self.map.get(id) {
            return handle;
        }
        let handle = Handle(self.strings.len() as u16);
        self.map.insert(id, handle);
        self.strings.push(id);
        handle
    }

    pub fn get(&self, id: &str) -> Option<Handle> {
        self.map.get(id).copied()
    }
}

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

pub struct ResolvedDevice<'a> {
    pub tag_name: &'a str,
    pub id_handle: Handle,
    pub component_handle: Handle,
    pub raw_node: ParsedNode<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum ActivePeripheral {
    I2c { sda: u8, scl: u8 },
    Spi,
    Gpio,
    Uart,
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveComponent {
    pub parent_peripheral: Handle,
    pub address: u8,
}

/// What kind of hardware a `$` reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeripheralKind {
    Gpio,
    I2c,
    Spi,
    Uart,
}

/// The `$` graph without handles, tables or lifetimes.
///
/// Phase 2 replaces [`RuntimeContext`] with this: lowering needs to answer
/// "does this id exist, and what kind of peripheral is it?", not to look up a
/// 128-slot integer table. The ids are owned, so the result can live next to
/// the parsed file it describes.
#[derive(Debug, Clone, Default)]
pub struct Refs {
    ids: HashSet<String>,
    peripherals: HashMap<String, PeripheralKind>,
}

impl Refs {
    /// Resolve every `$` reference and report the same graph problems the old
    /// `RuntimeContext` did, in the same order.
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
                        let span = peri
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
                        errors.push(span);
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

pub struct RuntimeContext<'a> {
    pub interner: IdInterner<'a>,
    pub active_peripherals: Vec<Option<ActivePeripheral>>,
    pub active_components: Vec<Option<ActiveComponent>>,
    pub resolved_devices: Vec<ResolvedDevice<'a>>,
}

impl<'a> RuntimeContext<'a> {
    pub fn new() -> Self {
        Self {
            interner: IdInterner::default(),
            active_peripherals: vec![None; 128],
            active_components: vec![None; 128],
            resolved_devices: Vec::new(),
        }
    }

    pub fn resolve_and_bind(
        &mut self,
        raw_peripherals: &[ParsedNode<'a>],
        raw_components: &[ParsedNode<'a>],
        raw_devices: &[ParsedNode<'a>],
    ) -> Result<(), Vec<crate::diag::EspfError>> {
        let mut errors = Vec::new();

        // 1. Map Peripherals
        for peri in raw_peripherals {
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
            let peri_handle = self.interner.get_or_insert(peri.id);
            let active = match peri.tag_name {
                "i2c" => {
                    let sda = parse_u8_attr(peri, "sda", "0", &mut errors);
                    let scl = parse_u8_attr(peri, "scl", "0", &mut errors);
                    match (sda, scl) {
                        (Some(sda), Some(scl)) => ActivePeripheral::I2c { sda, scl },
                        _ => continue,
                    }
                }
                "spi" => ActivePeripheral::Spi,
                "uart" => ActivePeripheral::Uart,
                _ => ActivePeripheral::Gpio,
            };
            self.active_peripherals[peri_handle.0 as usize] = Some(active);
        }

        // 2. Resolve Components
        for comp in raw_components {
            if comp.id.is_empty() {
                errors.push(crate::diag::EspfError::at_node(
                    comp,
                    "component is missing an id",
                    "<i2c-...> elements need an id=\"...\" attribute",
                ));
                continue;
            }
            let comp_handle = self.interner.get_or_insert(comp.id);

            // Search dynamically for any attribute referencing a parent connection
            let Some(bus_attr) = comp
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

            let parent_str = bus_attr.val.trim_start_matches('$');
            let Some(peri_handle) = self.interner.get(parent_str) else {
                errors.push(crate::diag::EspfError::at_attr(
                    bus_attr,
                    "unknown peripheral",
                    format!("there is no `{parent_str}` in `# peripherals`; the `$` reference must match an id defined there"),
                ));
                continue;
            };

            let addr = comp
                .get_attr("address")
                .map(|a| u8::from_str_radix(a.trim_start_matches("0x"), 16).unwrap_or(0))
                .unwrap_or(0);

            self.active_components[comp_handle.0 as usize] = Some(ActiveComponent {
                parent_peripheral: peri_handle,
                address: addr,
            });
        }

        // 3. Resolve Leaf Devices
        for dev in raw_devices {
            if dev.id.is_empty() {
                errors.push(crate::diag::EspfError::at_node(
                    dev,
                    "device is missing an id",
                    format!("<{}> needs an id=\"...\" attribute", dev.tag_name),
                ));
                continue;
            }
            let dev_handle = self.interner.get_or_insert(dev.id);

            let Some(comp_attr) = dev.attributes.iter().find(|a| a.key.contains("component"))
            else {
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

            let comp_str = comp_attr.val.trim_start_matches('$');
            let Some(comp_handle) = self.interner.get(comp_str) else {
                errors.push(crate::diag::EspfError::at_attr(
                    comp_attr,
                    "unknown component",
                    format!("there is no `{comp_str}` in `# components`; the `$` reference must match an id defined there"),
                ));
                continue;
            };

            self.resolved_devices.push(ResolvedDevice {
                tag_name: dev.tag_name,
                id_handle: dev_handle,
                component_handle: comp_handle,
                raw_node: dev.clone(),
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Parse a u8-valued attribute, recording a span-aware error instead of
/// panicking on malformed input. Returns `None` on failure so the caller can
/// skip binding that node.
fn parse_u8_attr(
    node: &ParsedNode,
    key: &str,
    default: &str,
    errors: &mut Vec<crate::diag::EspfError>,
) -> Option<u8> {
    let val = node.get_attr(key).unwrap_or(default);
    match val.parse::<u8>() {
        Ok(v) => Some(v),
        Err(_) => {
            errors.push(crate::diag::EspfError::at_attr(
                node.get_attr_spanned(key)
                    .unwrap_or_else(|| unreachable!("val came from get_attr")),
                "invalid pin number",
                format!("`{key}=\"{val}\"` must be a number in 0..=255"),
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{EspfError, line_number_for_offset};
    use crate::parser::parse_espf_runtime;

    fn compile(src: &str) -> Vec<ParsedNode> {
        let sections = parse_espf_runtime(src);
        [
            sections
                .get("peripherals")
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            sections.get("components").map(Vec::as_slice).unwrap_or(&[]),
            sections.get("devices").map(Vec::as_slice).unwrap_or(&[]),
        ]
        .concat()
    }

    /// A `$`-reference pointing at an unknown id is reported as a span-aware
    /// error pointing at the offending value, not a string blob.
    #[test]
    fn unresolved_reference_points_at_value() {
        let src = "# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <oled id=\"oled1\" i2c=\"$i2cX\" address=\"0x3c\" />\n";
        let all = compile(src);
        let (peris, comps) = (&all[..1], &all[1..]);
        let mut ctx = RuntimeContext::new();
        let Err(errs) = ctx.resolve_and_bind(peris, comps, &[]) else {
            panic!("expected resolution failure");
        };
        assert_eq!(errs.len(), 1, "one error: {errs:?}");
        let EspfError { title, span, .. } = &errs[0];
        assert_eq!(title, "unknown peripheral");
        assert_eq!(&src[span.start..span.end], "$i2cX");
        assert_eq!(line_number_for_offset(src, span.start), 4);
    }

    #[test]
    fn valid_graph_resolves_child_to_parent() {
        let src = "# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <oled id=\"oled1\" i2c=\"$i2c0\" address=\"0x3c\" />\n# devices\n  <smile id=\"smile1\" component=\"$oled1\" />\n";
        let all = compile(src);
        let (peris, comps, devs) = (&all[..1], &all[1..2], &all[2..]);
        let mut ctx = RuntimeContext::new();
        ctx.resolve_and_bind(peris, comps, devs)
            .expect("valid graph resolves");
        assert_eq!(ctx.resolved_devices.len(), 1);
        let comp_handle = ctx.resolved_devices[0].component_handle;
        assert_eq!(ctx.interner.get("oled1"), Some(comp_handle));
    }

    #[test]
    fn malformed_pin_value_is_an_error_not_a_panic() {
        let src = "# peripherals\n  <i2c id=\"i2c0\" sda=\"nine\" scl=\"5\" />\n";
        let peris = compile(src);
        let mut ctx = RuntimeContext::new();
        let Err(errs) = ctx.resolve_and_bind(&peris, &[], &[]) else {
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
        let all = compile(src);
        let mut ctx = RuntimeContext::new();
        let Err(errs) = ctx.resolve_and_bind(&[], &all[..1], &all[1..]) else {
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

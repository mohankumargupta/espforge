//! Generic device lowering: one interpreter for every `device_recipe`.
//!
//! A display and a sensor differ only in data the schema already carries: the
//! bus component they attach to, the pins they claim, and the Rust expression
//! each allowed attribute spelling emits. [`DeviceRecipeElement`] reads that
//! data and follows the same steps a hand-written device element did. It knows
//! no tag.
//!
//! The typed, schema-generated `Attrs` readers still own presence and kind:
//! [`crate::generated::validate`] runs first, so a missing or misspelled value
//! is refused with the same wording as before, and an unmapped spelling cannot
//! reach emission. `espforge-schema` proves at load time that every value the
//! schema allows has a mapping.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use espforge_schema::{AddressNeed, DeviceArg, DeviceRecipe};

use crate::attrs;
use crate::devices::{
    invalid_element, invalid_id, requires_blocking, span_of, unknown_component, unknown_gpio,
    wrong_direction,
};
use crate::element::{Element, LowerCx, Reported, is_rust_ident};
use crate::generated;
use crate::model::{
    ClaimedPin, Device, Direction, Owner, OwnerKind, Pin, PinUse, Runtime,
};

/// The interpreter for one schema element that declares a `device_recipe`.
pub struct DeviceRecipeElement {
    element: &'static espforge_schema::Element,
}

impl DeviceRecipeElement {
    /// An interpreter over `element`, which must declare a `device_recipe`.
    pub fn new(element: &'static espforge_schema::Element) -> Self {
        Self { element }
    }

    /// The schema element this recipe describes.
    fn element(&self) -> &'static espforge_schema::Element {
        self.element
    }

    /// The element's recipe. A `DeviceRecipeElement` is only ever built for an
    /// element that has one.
    fn recipe(&self) -> &'static DeviceRecipe {
        self.element
            .device_recipe
            .as_ref()
            .expect("a DeviceRecipeElement is only built for an element with a device_recipe")
    }
}

impl Element for DeviceRecipeElement {
    fn tag(&self) -> &'static str {
        self.element().tag.as_str()
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let tag = self.tag();
        let recipe = self.recipe();

        // 1. the schema's own readers: presence, kind and allowed values.
        match generated::validate(tag, node) {
            Some(Err(issue)) => {
                return Err(invalid_element(cx, node, tag, self.element(), issue));
            }
            Some(Ok(())) => {}
            None => panic!("`<{tag}>` has no generated `Attrs`"),
        }

        // 2. the id must become a Rust identifier.
        let id = cx.ident(node);
        if !is_rust_ident(&id.rust_name()) {
            return Err(invalid_id(cx, node, tag, &id));
        }

        // 3. the drivers this tier covers are blocking-only.
        if cx.runtime() != Runtime::Blocking {
            return Err(requires_blocking(cx, node, tag, &id, &recipe.kind));
        }

        // 4. the bus component, of the kind the schema says it accepts.
        let bus_attr = recipe.bus.as_str();
        let reference = attrs::reference(node, bus_attr)
            .map_err(|issue| invalid_element(cx, node, tag, self.element(), issue))?;
        let component_name = reference.name;
        let Some(component) = cx
            .components()
            .iter()
            .find(|component| component.id().raw == component_name)
        else {
            return Err(unknown_component(
                cx, node, tag, &id, bus_attr, &recipe.expect,
            ));
        };
        // The ids are the same text from different nodes, so their spans
        // differ: clone before the claims take `cx` mutably again.
        let component_tag = component.tag();
        let bus_component = component.id().clone();
        let component_address = component.address();
        if component_tag != recipe.expect {
            return Err(unknown_component(
                cx, node, tag, &id, bus_attr, &recipe.expect,
            ));
        }

        // 5. an addressed part needs an addressed component: a driver that
        // takes an `I2cDevice` cannot be handed a raw bus handle, and the
        // `.espf` file promises the compiler catches that, not `rustc`.
        match (recipe.address, component_address) {
            (AddressNeed::None, _) | (AddressNeed::Any, Some(_)) => {}
            (AddressNeed::Any, None) | (AddressNeed::Fixed(_), None) => {
                let fixed = match recipe.address {
                    AddressNeed::Fixed(fixed) => format!(" (this part is at 0x{fixed:02X})"),
                    _ => String::new(),
                };
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, bus_attr),
                    format!("{} needs an i2c address", recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> needs <{component_tag} id=\"{component_name}\"> to \
                         declare an `address`{fixed}; it is a bus-only component",
                        id.raw
                    ),
                ));
            }
            (AddressNeed::Fixed(want), Some(got)) if got != want => {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, bus_attr),
                    format!("{} address mismatch", recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> is fixed at address 0x{want:02X}, but \
                         <{component_tag} id=\"{component_name}\"> declares 0x{got:02X}",
                        id.raw
                    ),
                ));
            }
            (AddressNeed::Fixed(_), Some(_)) => {}
        }

        // 6. one device per bus component when the device takes the handle.
        if recipe.takes_bus {
            let taken = cx
                .devices()
                .iter()
                .find(|device| device.takes(&bus_component))
                .map(|device| device.id.raw.clone());
            if let Some(other) = taken {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    node_span(node),
                    format!("{} already used", recipe.expect.replace('_', " ")),
                    format!(
                        "<{tag} id=\"{other}\"> and <{tag} id=\"{}\"> both use <{} id=\"{component_name}\">",
                        id.raw, recipe.expect
                    ),
                ));
            }
        }

        // 7. the auxiliary control lines, claimed in recipe order.
        let mut pins: Vec<(String, ClaimedPin)> = Vec::new();
        for arg in &recipe.args {
            if arg.pin.is_none() {
                continue;
            }
            // An absent optional pin is not claimed; a required one was
            // already refused by `validate`.
            let Ok(reference) = attrs::reference(node, arg.name.as_str()) else {
                continue;
            };
            let Some(gpio) = cx.gpio(reference.name).cloned() else {
                return Err(unknown_gpio(cx, node, tag, &id, &arg.name, reference.name));
            };
            let wanted = direction(arg.direction);
            if let Some(found) = gpio.direction.filter(|found| *found != wanted) {
                return Err(wrong_direction(
                    cx, node, tag, &id, &arg.name, &gpio.id.raw, wanted, found,
                ));
            }
            let need = match wanted {
                Direction::Input => PinUse::Input,
                Direction::Output => PinUse::Output,
            };
            let claim = cx
                .claim_pin(
                    Pin(gpio.pin),
                    Owner::new(OwnerKind::Device(tag), id.clone()),
                    node_span(node),
                    need,
                )
                .map_err(|conflict| cx.report_conflict(&conflict))?;
            pins.push((arg.name.clone(), claim));
        }

        // 8. the constructor arguments, in recipe order.
        let args = arguments(node, self.element(), recipe, &pins);

        // 9. the field type, with its command slots filled.
        let ty = fill(&recipe.ty, &recipe.args, &args);

        cx.push_device(Device {
            id,
            tag,
            bus_component,
            feature: recipe.feature.clone(),
            deps: recipe.deps.clone(),
            ty,
            new: recipe.new.clone(),
            takes_bus: recipe.takes_bus,
            taken: recipe.taken.clone(),
            pins,
            args,
        });
        Ok(())
    }
}

/// The Rust expression each declared argument resolves to, in recipe order.
fn arguments(
    node: &ParsedNode<'_>,
    element: &espforge_schema::Element,
    recipe: &DeviceRecipe,
    pins: &[(String, ClaimedPin)],
) -> Vec<String> {
    recipe
        .args
        .iter()
        .map(|arg| match &arg.pin {
            // A pin slot is the claimed line, wrapped in `Some(..)` when the
            // attribute is optional: the driver's argument is an `Option`.
            Some(template) => match pins.iter().find(|(name, _)| *name == arg.name) {
                Some((_, pin)) => {
                    let expression = template.replace("{n}", &pin.pin().0.to_string());
                    if required(element, &arg.name) {
                        expression
                    } else {
                        format!("Some({expression})")
                    }
                }
                None => "None".to_string(),
            },
            // A mapped slot is the expression its spelling carries.
            None => {
                let spelling = spelling(node, element, &arg.name);
                arg.map
                    .as_ref()
                    .and_then(|map| {
                        map.iter()
                            .find(|(value, _)| value == spelling)
                            .map(|(_, expression)| expression.clone())
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "`<{}>` has no mapping for {}=\"{spelling}\"",
                            element.tag, arg.name
                        )
                    })
            }
        })
        .collect()
}

/// The board model's direction for one the schema declared.
fn direction(value: espforge_schema::Direction) -> Direction {
    match value {
        espforge_schema::Direction::Input => Direction::Input,
        espforge_schema::Direction::Output => Direction::Output,
    }
}

/// Whether the schema marks `name` required on `element`.
fn required(element: &espforge_schema::Element, name: &str) -> bool {
    element
        .attrs
        .iter()
        .any(|attr| attr.name == name && attr.required)
}

/// The value written for `name`, or the default the schema gives it.
fn spelling<'a>(
    node: &'a ParsedNode<'a>,
    element: &'a espforge_schema::Element,
    name: &str,
) -> &'a str {
    node.get_attr(name)
        .or_else(|| {
            element
                .attrs
                .iter()
                .find(|attr| attr.name == name)
                .and_then(|attr| attr.default.as_deref())
        })
        .unwrap_or_default()
}

/// `text` with every `{slot}` replaced by the value its argument resolved to.
fn fill(text: &str, names: &[DeviceArg], values: &[String]) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once('{') {
        out.push_str(head);
        let Some((slot, after)) = tail.split_once('}') else {
            out.push('{');
            rest = tail;
            continue;
        };
        let slot = slot.trim();
        match names.iter().position(|arg| arg.name == slot) {
            Some(index) => out.push_str(&values[index]),
            None => {
                out.push('{');
                out.push_str(slot);
                out.push('}');
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(name: &str) -> DeviceArg {
        DeviceArg {
            name: name.into(),
            map: None,
            pin: None,
            direction: espforge_schema::Direction::Output,
            span: 0..0,
        }
    }

    #[test]
    fn a_type_placeholder_takes_its_argument_value() {
        let names = [arg("size"), arg("rotation")];
        let values = ["espforge_drivers::Oled128x64".to_string(), "Rotate90".to_string()];
        assert_eq!(
            fill("espforge_drivers::Ssd1306Display<{size}>", &names, &values),
            "espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>"
        );
        // No placeholders, no change.
        assert_eq!(
            fill("espforge_drivers::Ili9341Display", &names, &values),
            "espforge_drivers::Ili9341Display"
        );
        // An unknown placeholder is left as written.
        assert_eq!(fill("Display<{nope}>", &names, &values), "Display<{nope}>");
    }
}

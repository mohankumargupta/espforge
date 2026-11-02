//! The element registry: one distributed slice, no hand-kept list.
//!
//! Each element module declares its own `#[distributed_slice(ELEMENTS)]`, so
//! registering an element never edits this file. `espforge-core` references
//! this crate explicitly, and the "registry is non-empty" test guards the
//! other failure mode: a slice the linker drops.
//!
//! There are three more doors, all keyed on data the schema owns: a component
//! with a `recipe` needs no Rust file, a device with a `device_recipe` needs
//! none either, and neither does a service with a `service_recipe`, so
//! [`element`] falls back to [`recipes`], [`device_recipes`] and
//! [`service_recipes`] in schema order. The coherence tests keep the four doors
//! from disagreeing: every element has exactly one implementation.

use std::sync::OnceLock;

use linkme::distributed_slice;

use crate::components::recipe::RecipeElement;
use crate::devices::recipe::DeviceRecipeElement;
use crate::element::Element;
use crate::schema::schema;
use crate::services::recipe::ServiceRecipeElement;

/// Every registered element.
///
/// The order is the linker's, not the schema's: callers that need schema
/// order walk the schema and look the element up by tag.
#[distributed_slice]
pub static ELEMENTS: [&'static dyn Element];

/// Every registered element, as a slice.
pub fn elements() -> &'static [&'static dyn Element] {
    &ELEMENTS
}

/// One [`RecipeElement`] per schema element that declares a `recipe`, in
/// schema order.
///
/// Built once from the schema: a recipe is data, and the element that reads it
/// is one struct that knows no tag.
pub fn recipes() -> &'static [RecipeElement] {
    static RECIPES: OnceLock<Vec<RecipeElement>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        schema()
            .elements
            .iter()
            .filter(|element| element.recipe.is_some())
            .filter(|element| matches!(element.section.as_str(), "peripherals" | "components"))
            .map(RecipeElement::new)
            .collect()
    })
}

/// One [`DeviceRecipeElement`] per schema element that declares a
/// `device_recipe`, in schema order.
///
/// The device tier: a driver whose wiring is data needs no Rust element, so
/// adding one edits the driver crate and nothing here.
pub fn device_recipes() -> &'static [DeviceRecipeElement] {
    static RECIPES: OnceLock<Vec<DeviceRecipeElement>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        schema()
            .elements
            .iter()
            .filter(|element| element.device_recipe.is_some())
            .map(DeviceRecipeElement::new)
            .collect()
    })
}

/// One [`ServiceRecipeElement`] per schema element that declares a
/// `service_recipe`, in schema order.
///
/// The service tier: a service whose wiring is data needs no Rust element, so
/// adding one edits the runtime and the schema and nothing here.
pub fn service_recipes() -> &'static [ServiceRecipeElement] {
    static RECIPES: OnceLock<Vec<ServiceRecipeElement>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        schema()
            .elements
            .iter()
            .filter(|element| element.service_recipe.is_some())
            .map(ServiceRecipeElement::new)
            .collect()
    })
}

/// The registered element for `tag`, if any: a hand-written element first,
/// then a component recipe, then a device recipe, then a service recipe.
pub fn element(tag: &str) -> Option<&'static dyn Element> {
    ELEMENTS
        .iter()
        .copied()
        .find(|element| element.tag() == tag)
        .or_else(|| {
            recipes()
                .iter()
                .find(|element| element.tag() == tag)
                .map(|element| element as &'static dyn Element)
        })
        .or_else(|| {
            device_recipes()
                .iter()
                .find(|element| element.tag() == tag)
                .map(|element| element as &'static dyn Element)
        })
        .or_else(|| {
            service_recipes()
                .iter()
                .find(|element| element.tag() == tag)
                .map(|element| element as &'static dyn Element)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_schema::{AttrDef, AttrKind};

    #[test]
    fn the_registry_is_not_empty() {
        // A `linkme` slice can be dead-stripped; this is the canary.
        assert!(
            !elements().is_empty(),
            "no elements are registered: the distributed slice was dropped"
        );
    }

    #[test]
    fn the_recipe_door_reaches_led() {
        let led = element("led").expect("a recipe element is registered");
        assert_eq!(led.tag(), "led");
        assert!(
            recipes().iter().any(|element| element.tag() == "led"),
            "`<led>` is a recipe, not a hand-written element"
        );
    }

    /// An element must have exactly one implementation: a hand-written
    /// `Element` in the slice, or a `recipe` / `device_recipe` in the schema —
    /// never both, never neither. Pure, so the "two doors" failure is
    /// exercised without adding a real element.
    fn implementation_problem(
        element: &espforge_schema::Element,
        hand_written: &[&str],
    ) -> Option<String> {
        let registered = hand_written
            .iter()
            .filter(|tag| **tag == element.tag.as_str())
            .count();
        let recipe = element.recipe.is_some()
            || element.device_recipe.is_some()
            || element.service_recipe.is_some();
        match (registered, recipe) {
            (1, false) | (0, true) => None,
            (0, false) => Some(format!(
                "`<{}>` has no implementation: add a `recipe` (a `device_recipe` for a \
                 device, a `service_recipe` for a service) to its schema block, or \
                 `espforge-elements/src/{}/{}.rs`",
                element.tag,
                element.section,
                element.tag.replace('-', "_"),
            )),
            _ => Some(format!(
                "`<{}>` has {registered} hand-written implementation(s) and a `recipe`: delete \
                 `espforge-elements/src/{}/{}.rs`, its schema block already has a `recipe`",
                element.tag,
                element.section,
                element.tag.replace('-', "_"),
            )),
        }
    }

    #[test]
    fn every_schema_element_has_exactly_one_implementation() {
        let schema = schema();
        let hand_written: Vec<&str> = elements().iter().map(|element| element.tag()).collect();
        for element in &schema.elements {
            // Only sections that lower have elements; `# espforge` is read by
            // the core reader, not by the registry.
            if !matches!(
                element.section.as_str(),
                "peripherals" | "components" | "devices" | "services"
            ) {
                continue;
            }
            if let Some(problem) = implementation_problem(element, &hand_written) {
                panic!("{problem}");
            }
        }
    }

    /// A component with a `recipe` *and* a registered implementation has two
    /// doors and must be caught, naming the file to delete.
    #[test]
    fn a_recipe_beside_a_rust_file_is_refused() {
        let schema = schema();
        let led = schema.element("led").expect("`<led>` is a schema element");
        let problem = implementation_problem(led, &["led"]).expect("two doors is a problem");
        assert!(
            problem.contains("delete `espforge-elements/src/components/led.rs`"),
            "{problem}"
        );
        assert!(problem.contains("already has a `recipe`"), "{problem}");
        // The recipe door alone is the whole implementation.
        assert!(implementation_problem(led, &[]).is_none());
    }

    /// An `AttrDef` of `kind`, for the shape predicate's own unit test.
    fn attr(name: &str, kind: AttrKind) -> AttrDef {
        AttrDef {
            name: name.into(),
            kind,
            required: false,
            default: None,
            range: None,
            help: None,
            span: 0..0,
        }
    }

    /// The shape a recipe describes: an `id`, exactly one `ref` and booleans.
    fn is_recipe_shaped(attrs: &[AttrDef]) -> bool {
        let mut idents = 0;
        let mut refs = 0;
        for attr in attrs {
            match attr.kind {
                AttrKind::Ident => idents += 1,
                AttrKind::Bool => {}
                AttrKind::Ref { .. } => refs += 1,
                _ => return false,
            }
        }
        idents == 1 && refs == 1
    }

    /// Why a `# components` element that is recipe-shaped but not a recipe is a
    /// problem, if it is. The predicate has its own unit test below, so the
    /// failure path is exercised without adding a real element.
    fn tier_problem(tag: &str, attrs: &[AttrDef], has_recipe: bool) -> Option<String> {
        (is_recipe_shaped(attrs) && !has_recipe).then(|| {
            format!(
                "`<{tag}>` is a recipe component: add `recipe …` to its schema block and delete \
                 `components/{tag}.rs`"
            )
        })
    }

    #[test]
    fn a_recipe_shaped_component_without_a_recipe_fails() {
        let recipe_shaped = [
            attr("id", AttrKind::Ident),
            attr(
                "gpio",
                AttrKind::Ref {
                    to: vec!["gpio".into()],
                },
            ),
            attr("active_low", AttrKind::Bool),
        ];
        assert!(is_recipe_shaped(&recipe_shaped));
        assert_eq!(
            tier_problem("buzzer", &recipe_shaped, false).unwrap(),
            "`<buzzer>` is a recipe component: add `recipe …` to its schema block and delete \
             `components/buzzer.rs`"
        );
        // A real recipe is not a problem.
        assert!(tier_problem("buzzer", &recipe_shaped, true).is_none());

        // A bus component with an integer address is out of shape.
        let bus = [
            attr("id", AttrKind::Ident),
            attr(
                "i2c",
                AttrKind::Ref {
                    to: vec!["i2c".into()],
                },
            ),
            attr("address", AttrKind::U32),
        ];
        assert!(!is_recipe_shaped(&bus));
        assert!(tier_problem("screen", &bus, false).is_none());

        // Two refs is not one pin.
        let two_refs = [
            attr("id", AttrKind::Ident),
            attr(
                "a",
                AttrKind::Ref {
                    to: vec!["gpio".into()],
                },
            ),
            attr(
                "b",
                AttrKind::Ref {
                    to: vec!["gpio".into()],
                },
            ),
        ];
        assert!(!is_recipe_shaped(&two_refs));
    }

    /// The real schema has to obey the tier: every recipe-shaped component is a
    /// recipe.
    #[test]
    fn every_recipe_shaped_component_declares_a_recipe() {
        let schema = schema();
        for element in schema.elements_in("components") {
            if let Some(problem) =
                tier_problem(&element.tag, &element.attrs, element.recipe.is_some())
            {
                panic!("{problem}");
            }
        }
    }

    /// The device door is open: a `device_recipe` element is found without a
    /// Rust file, in schema order after the component recipes.
    #[test]
    fn the_device_door_reaches_every_recipe_driver() {
        for tag in ["ssd1306", "ili9341", "scd4x"] {
            let registered = element(tag).unwrap_or_else(|| panic!("`<{tag}>` has an implementation"));
            assert_eq!(registered.tag(), tag);
            assert!(
                device_recipes().iter().any(|recipe| recipe.tag() == tag),
                "`<{tag}>` comes from its device recipe"
            );
            let schema_element = schema().element(tag).unwrap();
            assert!(
                implementation_problem(schema_element, &[]).is_none(),
                "`<{tag}>` has exactly one implementation"
            );
        }
    }

    /// A device with a `device_recipe` *and* a registered implementation has
    /// two doors, and the coherence test names the file to delete.
    #[test]
    fn a_device_recipe_beside_a_rust_file_is_refused() {
        let schema = schema();
        let ili = schema
            .element("ili9341")
            .expect("`<ili9341>` is a schema element");
        let problem = implementation_problem(ili, &["ili9341"]).expect("two doors is a problem");
        assert!(
            problem.contains("delete `espforge-elements/src/devices/ili9341.rs`"),
            "{problem}"
        );
        assert!(problem.contains("already has a `recipe`"), "{problem}");
        assert!(implementation_problem(ili, &[]).is_none());
    }

    /// The service door is open: a `service_recipe` element is found without a
    /// Rust file, in schema order after the device recipes.
    #[test]
    fn the_service_door_reaches_every_recipe_service() {
        for tag in ["http_client", "https_client", "ws_client", "wss_client", "ntp"] {
            let registered =
                element(tag).unwrap_or_else(|| panic!("`<{tag}>` has an implementation"));
            assert_eq!(registered.tag(), tag);
            assert!(
                service_recipes().iter().any(|recipe| recipe.tag() == tag),
                "`<{tag}>` comes from its service recipe"
            );
            let schema_element = schema().element(tag).unwrap();
            assert!(
                implementation_problem(schema_element, &[]).is_none(),
                "`<{tag}>` has exactly one implementation"
            );
        }
    }

    /// A service with a `service_recipe` *and* a registered implementation has
    /// two doors, and the coherence test names the file to delete.
    #[test]
    fn a_service_recipe_beside_a_rust_file_is_refused() {
        let schema = schema();
        let ntp = schema.element("ntp").expect("`<ntp>` is a schema element");
        let problem = implementation_problem(ntp, &["ntp"]).expect("two doors is a problem");
        assert!(
            problem.contains("delete `espforge-elements/src/services/ntp.rs`"),
            "{problem}"
        );
        assert!(implementation_problem(ntp, &[]).is_none());
    }

    #[test]
    fn every_registered_element_is_a_schema_element() {
        let schema = schema();
        for element in elements() {
            let tag = element.tag();
            let schema_element = schema.element(tag).unwrap_or_else(|| {
                panic!(
                    "`<{tag}>` is registered but not in espf.schema.kdl: add it to the schema \
                     or delete the element from espforge-elements/src"
                )
            });
            assert!(
                matches!(
                    schema_element.section.as_str(),
                    "peripherals" | "components" | "devices" | "services"
                ),
                "`<{tag}>` is registered from a `<{}>` schema element, but only \
                 peripherals, components, devices and services lower",
                schema_element.section
            );
        }
    }
}

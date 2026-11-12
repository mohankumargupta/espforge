//! The element registry: one distributed slice, no hand-kept list.
//!
//! Each element module declares its own `#[distributed_slice(ELEMENTS)]`, so
//! registering an element never edits this file. `espforge-core` references
//! this crate explicitly, and the "registry is non-empty" test guards the
//! other failure mode: a slice the linker drops.
//!
//! The recipe doors — a pin device with a `recipe`, a device with a
//! `device_recipe`, a service with a `service_recipe` — are not process-wide:
//! they are built from the schema a [`Session`](crate::schema::Session) owns,
//! so two sessions generated in one process cannot disagree about a tag. The
//! coherence tests keep the four doors apart: every element has exactly one
//! implementation.

use linkme::distributed_slice;

use crate::element::Element;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Session, schema};

    #[test]
    fn the_registry_is_not_empty() {
        // A `linkme` slice can be dead-stripped; this is the canary.
        assert!(
            !elements().is_empty(),
            "no elements are registered: the distributed slice was dropped"
        );
    }

    #[test]
    fn the_pin_recipe_door_reaches_led() {
        let session = Session::bundled();
        let led = session
            .element("led")
            .expect("a pin recipe element is registered");
        assert_eq!(led.tag(), "led");
        assert!(
            session
                .pin_recipes()
                .iter()
                .any(|element| element.tag() == "led"),
            "`<led>` is a pin recipe, not a hand-written element"
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
                "peripherals" | "devices" | "services"
            ) {
                continue;
            }
            if let Some(problem) = implementation_problem(element, &hand_written) {
                panic!("{problem}");
            }
        }
    }

    /// A pin device with a `recipe` *and* a registered implementation has two
    /// doors and must be caught, naming the file to delete.
    #[test]
    fn a_pin_recipe_beside_a_rust_file_is_refused() {
        let schema = schema();
        let led = schema.element("led").expect("`<led>` is a schema element");
        let problem = implementation_problem(led, &["led"]).expect("two doors is a problem");
        assert!(
            problem.contains("delete `espforge-elements/src/devices/led.rs`"),
            "{problem}"
        );
        assert!(problem.contains("already has a `recipe`"), "{problem}");
        // The recipe door alone is the whole implementation.
        assert!(implementation_problem(led, &[]).is_none());
    }

    /// The device door is open: a `device_recipe` element is found without a
    /// Rust file, in schema order after the pin recipes.
    #[test]
    fn the_device_door_reaches_every_recipe_driver() {
        let session = Session::bundled();
        for tag in ["ssd1306", "ili9341", "scd4x"] {
            let registered = session
                .element(tag)
                .unwrap_or_else(|| panic!("`<{tag}>` has an implementation"));
            assert_eq!(registered.tag(), tag);
            assert!(
                session
                    .device_recipes()
                    .iter()
                    .any(|recipe| recipe.tag() == tag),
                "`<{tag}>` comes from its device recipe"
            );
            let schema_element = session.schema().element(tag).unwrap();
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
        let session = Session::bundled();
        for tag in [
            "http_client",
            "https_client",
            "ws_client",
            "wss_client",
            "ntp",
        ] {
            let registered = session
                .element(tag)
                .unwrap_or_else(|| panic!("`<{tag}>` has an implementation"));
            assert_eq!(registered.tag(), tag);
            assert!(
                session
                    .service_recipes()
                    .iter()
                    .any(|recipe| recipe.tag() == tag),
                "`<{tag}>` comes from its service recipe"
            );
            let schema_element = session.schema().element(tag).unwrap();
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
                    "peripherals" | "devices" | "services"
                ),
                "`<{tag}>` is registered from a `<{}>` schema element, but only \
                 peripherals, devices and services lower",
                schema_element.section
            );
        }
    }
}

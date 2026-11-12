//! The composed schema: the core vocabulary, its services and the drivers'
//! own devices — plus, at runtime, a project's own driver manifests.
//!
//! `espforge-schema` owns `espf.schema.kdl` and the `services.schema.kdl`
//! beside it; `espforge-drivers` owns `drivers.schema.kdl` beside its drivers.
//! Neither crate reaches into the other's package, so both stay publishable:
//! composing is this crate's job, and every consumer here goes through a
//! [`Session`] rather than parsing its own subset.
//!
//! A project may add driver manifests (`drivers/*.kdl`, `--driver-dir`). Those
//! are read once per run and composed into a [`Session`]: the schema is a value
//! owned by the run, and the interpreters built from it borrow from that value.
//! Nothing is installed in a process global and nothing is leaked to get a
//! `'static`, so two projects generated in one process cannot see each other's
//! drivers.
//!
//! The one process-wide schema left is [`schema`]: the bundled files, which are
//! the same for every project.

use std::borrow::Cow;
use std::sync::OnceLock;

use espforge_schema::Schema;

use crate::devices::pin::PinRecipeElement;
use crate::devices::recipe::DeviceRecipeElement;
use crate::element::Element;
use crate::registry;
use crate::services::recipe::ServiceRecipeElement;

/// The bundled sources: the core vocabulary, the services, and one file per
/// bundled driver.
fn bundled_sources() -> Vec<(String, String)> {
    let mut sources = vec![
        (
            "bundled espf.schema.kdl".to_string(),
            espforge_schema::SCHEMA_KDL.to_string(),
        ),
        (
            "bundled services.schema.kdl".to_string(),
            espforge_schema::SERVICES_SCHEMA_KDL.to_string(),
        ),
    ];
    sources.extend(
        espforge_drivers::MANIFESTS
            .iter()
            .map(|(label, text)| (format!("bundled {label}"), (*text).to_string())),
    );
    sources
}

/// Every element the bundled schema knows, parsed once.
///
/// This is **the bundled** schema: the core vocabulary, the services and the
/// bundled drivers. It is the same for every project, so it is the one schema
/// that may live in a process global. A project's own drivers are not here;
/// they live in the [`Session`] that composed them.
pub fn schema() -> &'static Schema {
    bundled()
}

fn bundled() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        compose(&[]).unwrap_or_else(|errors| {
            let mut text = String::from("the composed schema does not load:");
            for error in errors {
                text.push_str(&format!("\n  {}: {}", error.span.start, error.message));
            }
            panic!("{text}")
        })
    })
}

/// The bundled sources followed by `extra`, in precedence order.
///
/// `extra` is `(label, text)` where the label is the path a user can open; a
/// tag declared twice is refused with both labels. This is the same composition
/// the build script does, with project manifests appended.
pub fn compose(extra: &[(String, String)]) -> Result<Schema, Vec<espforge_schema::SchemaError>> {
    let mut sources = bundled_sources();
    sources.extend(extra.iter().cloned());
    espforge_schema::load_composed(&sources)
}

/// One run's schema: the bundled files plus whatever the project added.
///
/// `Cow` because the common case is "no project manifests": the bundled schema
/// is borrowed from the process-wide cache, and only a project that adds
/// drivers pays for an owned copy. There is no install step and no leak, and the
/// recipe interpreters store tags rather than `&'static` elements, so nothing
/// borrows from the schema.
pub struct Session {
    schema: Cow<'static, Schema>,
    /// One interpreter per schema element that declares a recipe, in schema
    /// order.
    pin_recipes: Vec<PinRecipeElement>,
    /// One interpreter per schema element that declares a `device_recipe`.
    device_recipes: Vec<DeviceRecipeElement>,
    /// One interpreter per schema element that declares a `service_recipe`.
    service_recipes: Vec<ServiceRecipeElement>,
}

impl Session {
    /// The bundled schema, with no project manifests.
    pub fn bundled() -> Self {
        Self::from_schema(Cow::Borrowed(bundled()))
    }

    /// The bundled schema followed by `extra`, composed exactly as [`compose`]
    /// does. An empty `extra` borrows the bundled schema instead of rebuilding
    /// it.
    pub fn compose(extra: &[(String, String)]) -> Result<Self, Vec<espforge_schema::SchemaError>> {
        if extra.is_empty() {
            return Ok(Self::bundled());
        }
        Ok(Self::from_schema(Cow::Owned(compose(extra)?)))
    }

    fn from_schema(schema: Cow<'static, Schema>) -> Self {
        let pin_recipes = schema
            .elements
            .iter()
            .filter(|element| element.recipe.is_some())
            .filter(|element| matches!(element.section.as_str(), "peripherals" | "devices"))
            .map(|element| PinRecipeElement::new(element.tag.clone()))
            .collect();
        let device_recipes = schema
            .elements
            .iter()
            .filter(|element| element.device_recipe.is_some())
            .map(|element| DeviceRecipeElement::new(element.tag.clone()))
            .collect();
        let service_recipes = schema
            .elements
            .iter()
            .filter(|element| element.service_recipe.is_some())
            .map(|element| ServiceRecipeElement::new(element.tag.clone()))
            .collect();
        Self {
            schema,
            pin_recipes,
            device_recipes,
            service_recipes,
        }
    }

    /// The schema this run lowers against.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The pin interpreters this session built, in schema order.
    pub fn pin_recipes(&self) -> &[PinRecipeElement] {
        &self.pin_recipes
    }

    /// The device interpreters this session built, in schema order.
    pub fn device_recipes(&self) -> &[DeviceRecipeElement] {
        &self.device_recipes
    }

    /// The service interpreters this session built, in schema order.
    pub fn service_recipes(&self) -> &[ServiceRecipeElement] {
        &self.service_recipes
    }

    /// The registered element for `tag`, if any: a hand-written element first,
    /// then this session's pin recipe, then its device recipe, then its service
    /// recipe.
    pub fn element(&self, tag: &str) -> Option<&dyn Element> {
        if let Some(registered) = registry::elements()
            .iter()
            .copied()
            .find(|element| element.tag() == tag)
        {
            return Some(registered);
        }
        self.pin_recipes
            .iter()
            .find(|element| element.tag() == tag)
            .map(|element| element as &dyn Element)
            .or_else(|| {
                self.device_recipes
                    .iter()
                    .find(|element| element.tag() == tag)
                    .map(|element| element as &dyn Element)
            })
            .or_else(|| {
                self.service_recipes
                    .iter()
                    .find(|element| element.tag() == tag)
                    .map(|element| element as &dyn Element)
            })
    }

    /// The tags that can appear in `section`: schema elements with an
    /// implementation, in schema order.
    pub fn known_tags(&self, section: &str) -> Vec<String> {
        self.schema
            .elements_in(section)
            .filter(|element| self.element(&element.tag).is_some())
            .map(|element| element.tag.clone())
            .collect()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::bundled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_schema::schema as core_schema;

    #[test]
    fn the_drivers_schema_joins_the_core_one() {
        let composed = schema();
        // The devices arrive from `espforge-drivers`, the section from the
        // core schema.
        for tag in ["ssd1306", "ili9341", "scd4x", "adxl345"] {
            let element = composed
                .element(tag)
                .unwrap_or_else(|| panic!("`<{tag}>` is in the composed schema"));
            assert_eq!(element.section, "devices");
            assert!(
                element.device_recipe.is_some(),
                "`<{tag}>` is a device recipe"
            );
            assert!(
                element.source.as_deref().is_some(),
                "`<{tag}>` records the source it came from"
            );
        }
        assert!(composed.section_named("devices").is_some());
        // Everything the core schema declares is still there.
        for element in &core_schema().elements {
            assert!(
                composed.element(&element.tag).is_some(),
                "`<{}>` survived composition",
                element.tag
            );
        }
        assert_eq!(
            composed.elements.len(),
            core_schema().elements.len() + 4,
            "the four driver elements are the only addition"
        );
    }

    #[test]
    fn a_device_recipe_reads_its_bus_and_slots() {
        let schema = schema();
        let ssd1306 = schema.element("ssd1306").unwrap();
        let recipe = ssd1306.device_recipe.as_ref().unwrap();
        assert_eq!(recipe.bus, "i2c");
        assert_eq!(recipe.feature, "ssd1306");
        assert_eq!(recipe.kind, "display");
        assert_eq!(recipe.ty, "espforge_drivers::Ssd1306Display<{size}>");
        assert_eq!(recipe.new, "espforge_drivers::Ssd1306Display::new");
        assert_eq!(recipe.deps, ["embedded-graphics"]);
        // A display declares its own address in the `.espf`; any usable one.
        assert_eq!(recipe.address, espforge_schema::AddressNeed::Any);
        let args: Vec<&str> = recipe.args.iter().map(|arg| arg.name.as_str()).collect();
        assert_eq!(args, ["size", "rotation"]);
        assert!(recipe.args[0].pin.is_none());
        assert_eq!(recipe.args[0].map.as_ref().unwrap().len(), 2);

        // The SPI display takes its bus and templates its control lines.
        let ili9341 = schema.element("ili9341").unwrap();
        let recipe = ili9341.device_recipe.as_ref().unwrap();
        assert_eq!(recipe.bus, "spi");
        assert_eq!(recipe.address, espforge_schema::AddressNeed::None);
        let pins: Vec<&str> = recipe
            .args
            .iter()
            .filter(|arg| arg.pin.is_some())
            .map(|arg| arg.name.as_str())
            .collect();
        assert_eq!(pins, ["dc", "reset", "backlight"]);
        assert!(
            recipe.args[2].pin.as_deref().unwrap().contains("{n}"),
            "a pin slot substitutes the claimed pin number"
        );
    }

    /// The sensor has no dependency of its own, so a project using it must not
    /// pull the graphics crate the displays need.
    #[test]
    fn the_sensor_recipe_names_no_dependency() {
        let scd4x = schema().element("scd4x").unwrap();
        assert!(scd4x.device_recipe.as_ref().unwrap().deps.is_empty());
    }

    /// The SCD4x answers at one address only, and the schema is the single
    /// place that says so: lowering checks the device against it.
    #[test]
    fn the_sensor_recipe_fixes_its_address() {
        let scd4x = schema().element("scd4x").unwrap();
        assert_eq!(
            scd4x.device_recipe.as_ref().unwrap().address,
            espforge_schema::AddressNeed::Fixed(0x62)
        );
    }

    /// A project manifest is composed in after the bundled files, and its
    /// element arrives with the label of the file it was read from.
    #[test]
    fn a_project_manifest_joins_the_composition() {
        let manifest = "\
element \"demo\" section=\"devices\" {
    summary \"A test driver\"
    device_recipe {
        bus \"i2c\"
        feature \"demo\"
        kind \"sensor\"
        type \"demo::Demo\"
        new \"demo::Demo::new\"
        needs_address
    }
    attr \"id\" kind=\"ident\" required=#true
    attr \"bus\" kind=\"ref\" to=\"i2c\" required=#true
    attr \"address\" kind=\"hex\" required=#true
    example \"minimal\" #\"<demo id=\"d\" bus=\"$i2c0\" address=\"0x10\" />\"#
}
"
        .to_string();
        let composed = compose(&[("drivers/demo.kdl".to_string(), manifest)]).unwrap();
        let demo = composed.element("demo").expect("the project driver loaded");
        assert_eq!(demo.source.as_deref(), Some("drivers/demo.kdl"));
        assert_eq!(
            demo.device_recipe.as_ref().unwrap().feature,
            "demo",
            "a package-less manifest keeps the bundled feature form"
        );
    }

    /// Two manifests claiming one tag are refused, naming both files.
    #[test]
    fn a_duplicate_tag_names_both_sources() {
        let manifest = |summary: &str| {
            format!(
                "element \"demo\" section=\"devices\" {{\n    summary \"{summary}\"\n    \
                 device_recipe {{\n        bus \"i2c\"\n        feature \"demo\"\n        kind \"sensor\"\n        \
                 type \"demo::Demo\"\n        new \"demo::Demo::new\"\n    }}\n    \
                 attr \"id\" kind=\"ident\" required=#true\n    \
                 attr \"bus\" kind=\"ref\" to=\"i2c\" required=#true\n    \
                 example \"minimal\" #\"<demo id=\"d\" bus=\"$i2c0\" />\"#\n}}\n"
            )
        };
        let errors = compose(&[
            ("drivers/demo.kdl".to_string(), manifest("one")),
            ("vendor/demo.kdl".to_string(), manifest("two")),
        ])
        .err()
        .expect("a duplicate tag is refused");
        let message = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(message.contains("duplicate tag `demo`"), "{message}");
        assert!(message.contains("drivers/demo.kdl"), "{message}");
        assert!(message.contains("vendor/demo.kdl"), "{message}");
    }

    /// A session with no project manifests borrows the bundled schema: it must
    /// not rebuild it, and it must resolve the same elements.
    #[test]
    fn an_empty_composition_is_the_bundled_session() {
        let session = Session::compose(&[]).unwrap();
        assert!(
            std::ptr::eq(session.schema(), schema()),
            "the bundled schema is borrowed, not rebuilt"
        );
        assert_eq!(session.element("led").unwrap().tag(), "led");
        assert!(
            session
                .known_tags("devices")
                .contains(&"ssd1306".to_string())
        );
    }

    /// The interpreters are built from this session's schema, so one session
    /// never answers with another's driver.
    #[test]
    fn a_second_session_reads_its_own_elements() {
        let a = Session::compose(&[("a/demo.kdl".to_string(), demo("demo_a"))]).unwrap();
        let b = Session::compose(&[("b/demo.kdl".to_string(), demo("demo_b"))]).unwrap();

        assert_eq!(
            a.element("demo_a").expect("session A's driver").tag(),
            "demo_a"
        );
        assert_eq!(
            b.element("demo_b").expect("session B's driver").tag(),
            "demo_b"
        );
        assert!(
            a.element("demo_b").is_none(),
            "session A must not see session B's driver"
        );
        assert!(
            b.element("demo_a").is_none(),
            "session B must not see session A's driver"
        );
        assert!(a.schema().element("demo_b").is_none());
        assert!(b.schema().element("demo_a").is_none());
    }

    /// A minimal project driver, so a session can be built without a fixture.
    fn demo(tag: &str) -> String {
        format!(
            "element \"{tag}\" section=\"devices\" {{\n    summary \"A test driver\"\n    \
             device_recipe {{\n        bus \"i2c\"\n        feature \"{tag}\"\n        kind \"sensor\"\n        \
             type \"demo::Demo\"\n        new \"demo::Demo::new\"\n    }}\n    \
             attr \"id\" kind=\"ident\" required=#true\n    \
             attr \"bus\" kind=\"ref\" to=\"i2c\" required=#true\n    \
             example \"minimal\" #\"<{tag} id=\"d\" bus=\"$i2c0\" />\"#\n}}\n"
        )
    }
}

//! The composed schema: the core vocabulary, its services and the drivers'
//! own devices.
//!
//! `espforge-schema` owns `espf.schema.kdl` and the `services.schema.kdl`
//! beside it; `espforge-drivers` owns `drivers.schema.kdl` beside its drivers.
//! Neither crate reaches into the other's package, so both stay publishable:
//! composing is this crate's job, and every consumer here goes through
//! [`schema`] rather than parsing its own subset.

use espforge_schema::Schema;
use std::sync::LazyLock;

/// Every element the pipeline knows, parsed once.
///
/// A schema that fails to load is a bug in the workspace, not in a user's
/// file: the crate's own tests parse it before it can ever be shipped.
pub fn schema() -> &'static Schema {
    static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
        espforge_schema::load_all(&[
            espforge_schema::SCHEMA_KDL,
            espforge_schema::SERVICES_SCHEMA_KDL,
            espforge_drivers::DRIVERS_SCHEMA_KDL,
        ])
        .unwrap_or_else(|errors| {
            let mut text = String::from("the composed schema does not load:");
            for error in errors {
                text.push_str(&format!("\n  {}: {}", error.span.start, error.message));
            }
            panic!("{text}")
        })
    });
    &SCHEMA
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
        for tag in ["ssd1306", "ili9341", "scd4x"] {
            let element = composed
                .element(tag)
                .unwrap_or_else(|| panic!("`<{tag}>` is in the composed schema"));
            assert_eq!(element.section, "devices");
            assert!(
                element.device_recipe.is_some(),
                "`<{tag}>` is a device recipe"
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
            core_schema().elements.len() + 3,
            "the three driver elements are the only addition"
        );
    }

    #[test]
    fn a_device_recipe_reads_its_bus_and_slots() {
        let schema = schema();
        let ssd1306 = schema.element("ssd1306").unwrap();
        let recipe = ssd1306.device_recipe.as_ref().unwrap();
        assert_eq!(recipe.bus, "i2c_component");
        assert_eq!(recipe.expect, "i2c_component");
        assert_eq!(recipe.feature, "ssd1306");
        assert_eq!(recipe.kind, "display");
        assert_eq!(recipe.ty, "espforge_drivers::Ssd1306Display<{size}>");
        assert_eq!(recipe.new, "espforge_drivers::Ssd1306Display::new");
        assert!(!recipe.takes_bus);
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
        assert_eq!(recipe.expect, "spi_component");
        assert!(recipe.takes_bus);
        assert_eq!(recipe.address, espforge_schema::AddressNeed::None);
        assert_eq!(
            recipe.taken.as_deref(),
            Some("the display's SPI bus was already taken")
        );
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
    /// place that says so: lowering checks the component against it.
    #[test]
    fn the_sensor_recipe_fixes_its_address() {
        let scd4x = schema().element("scd4x").unwrap();
        assert_eq!(
            scd4x.device_recipe.as_ref().unwrap().address,
            espforge_schema::AddressNeed::Fixed(0x62)
        );
    }
}

//! The element side of the pipeline: the core section reader, the option
//! resolver and the board IR the emitters consume.
//!
//! `core` reads `project`/`chip`/`runtime`/`console`/`options`, `options`
//! resolves the schema's option table, and [`model::Board`] carries the result
//! with the pins its devices claimed. [`generated`] holds the typed
//! attribute readers `build.rs` writes from the schema.

pub mod attrs;
pub mod bus_emit;
pub mod chips;
pub mod core;
pub mod devices;
pub mod dynamic_attrs;
pub mod element;
pub mod lower;
pub mod peripherals;
pub mod pin_caps;
pub mod raw_bus;
pub mod registry;
pub mod schema;

/// One `Attrs` struct per element in `espf.schema.kdl`, written by `build.rs`.
///
/// Included in a module body rather than declared as a `src/`-backed module:
/// the file lives in `OUT_DIR`, so a build never dirties the working tree.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/element_attrs.rs"));
}

pub mod model;
pub mod options;
pub mod services;

pub use core::core_from_section;
pub use element::{Element, EmitCx, LowerCx, Lowered, MainCtx, PinRefusal, Reported};
pub use espforge_schema::{Compat, Kind, OptValue, OptionDef, Requires, Schema};
pub use lower::lower;
pub use model::{
    Board, BusDevice, BusId, BusMode, BusTransport, Claim, ClaimedPin, Conflict, ConflictKind,
    Console, Device, Direction, Driver, GpioDecl, HeldClash, HeldResource, I2cTransport, Ident,
    Meta, Owner, OwnerKind, PeripheralRefusal, Pin, PinDevice, PinDeviceKind, PinUse,
    ProtocolService, ResourceGraph, Runtime, RuntimeSet, Service, SpiTransport,
};
pub use options::{
    Answer, Answers, DerivedInputs, Origin, Resolved, ResolvedOptions, Why, resolve, resolve_with,
};
pub use registry::elements;
pub use schema::Session;

#[cfg(test)]
mod tests {
    use crate::attrs::Issue;
    use crate::generated::{gpio, led};
    use espf::context::ParsedNode;

    fn node<'a>(section: &str, src: &'a str) -> ParsedNode<'a> {
        espf::parser::parse_espf(src).sections[section][0].clone()
    }

    #[test]
    fn a_gpio_node_reads_through_its_generated_attrs() {
        let src = "# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n";
        let n = node("peripherals", src);
        let attrs = gpio::Attrs::from_node(&n).unwrap();
        assert_eq!(attrs.id.value, "gpio18");
        assert_eq!(attrs.pin.value, 18);
        assert_eq!(attrs.direction.unwrap().value, gpio::Direction::Output);
        assert_eq!(gpio::Direction::Output.as_str(), "output");
    }

    #[test]
    fn a_gpio_without_a_direction_leaves_it_absent() {
        let src = "# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" />\n";
        let n = node("peripherals", src);
        let attrs = gpio::Attrs::from_node(&n).unwrap();
        assert!(attrs.direction.is_none());
    }

    #[test]
    fn a_missing_pin_is_an_issue_not_a_panic() {
        let src = "# peripherals\n  <gpio id=\"gpio18\" />\n";
        let n = node("peripherals", src);
        assert_eq!(
            gpio::Attrs::from_node(&n).unwrap_err(),
            Issue::Missing { attr: "pin" }
        );
    }

    #[test]
    fn a_bad_direction_reports_the_value_as_written() {
        let src = "# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"sideways\" />\n";
        let n = node("peripherals", src);
        assert_eq!(
            gpio::Attrs::from_node(&n).unwrap_err(),
            Issue::Invalid {
                attr: "direction",
                value: "sideways"
            }
        );
        assert_eq!(gpio::Direction::parse("sideways"), None);
        assert_eq!(
            gpio::Direction::CHOICES,
            ["output", "input", "bidirectional"]
        );
    }

    #[test]
    fn led_reads_its_reference_and_defaults_active_low_off() {
        let src = "# devices\n  <led id=\"red_led\" gpio=\"$gpio18\" />\n";
        let n = node("devices", src);
        let attrs = led::Attrs::from_node(&n).unwrap();
        assert_eq!(attrs.id.value, "red_led");
        assert_eq!(attrs.gpio.expect("a gpio reference").name, "gpio18");
        assert!(!attrs.active_low.value);
        // Defaulted at the node, not at some unrelated offset.
        assert_eq!(attrs.active_low.span.start, n.offset);
    }

    #[test]
    fn a_led_may_name_its_pin_by_number_instead_of_a_gpio() {
        let src = "# devices\n  <led id=\"red_led\" pin=\"18\" />\n";
        let n = node("devices", src);
        let attrs = led::Attrs::from_node(&n).expect("pin= is enough");
        assert_eq!(attrs.pin.expect("a pin number").value, 18);
        assert!(attrs.gpio.is_none());

        // Neither is a lowering error the recipe reports, not a parse error.
        let src = "# devices\n  <led id=\"red_led\" />\n";
        let n = node("devices", src);
        let attrs = led::Attrs::from_node(&n).expect("both pin attributes are optional");
        assert!(attrs.pin.is_none());
        assert!(attrs.gpio.is_none());
    }
}

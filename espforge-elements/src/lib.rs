//! The element side of the pipeline: the core section reader, the option
//! resolver and the board IR the emitters consume.
//!
//! `core` reads `project`/`chip`/`runtime`/`console`/`options`, `options`
//! resolves the schema's option table, and [`model::Board`] carries the result
//! with the pins its components claimed. [`generated`] holds the typed
//! attribute readers `build.rs` writes from the schema.

pub mod attrs;
pub mod chips;
pub mod components;
pub mod core;
pub mod devices;
pub mod element;
pub mod lower;
pub mod peripherals;
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
pub use element::{Element, EmitCx, LowerCx, Lowered, MainCtx, Reported};
pub use espforge_schema::{Compat, Kind, OptValue, OptionDef, Requires, Schema};
pub use lower::lower;
pub use model::{
    Board, Claim, ClaimedPin, Component, Conflict, ConflictKind, Console, Device, Direction,
    GpioDecl, Ident, Meta, Owner, OwnerKind, Pin, PinUse, ResourceGraph, Runtime, Service,
};
pub use options::{
    Answer, Answers, DerivedInputs, Origin, Resolved, ResolvedOptions, Why, resolve, resolve_with,
};
pub use registry::elements;

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
        assert_eq!(gpio::Direction::CHOICES, ["output", "input"]);
    }

    #[test]
    fn led_reads_its_reference_and_defaults_active_low_off() {
        let src = "# components\n  <led id=\"red_led\" gpio=\"$gpio18\" />\n";
        let n = node("components", src);
        let attrs = led::Attrs::from_node(&n).unwrap();
        assert_eq!(attrs.id.value, "red_led");
        assert_eq!(attrs.gpio.name, "gpio18");
        assert!(!attrs.active_low.value);
        // Defaulted at the node, not at some unrelated offset.
        assert_eq!(attrs.active_low.span.start, n.offset);
    }

    #[test]
    fn a_missing_led_reference_is_an_issue() {
        let src = "# components\n  <led id=\"red_led\" />\n";
        let n = node("components", src);
        assert_eq!(
            led::Attrs::from_node(&n).unwrap_err(),
            Issue::Missing { attr: "gpio" }
        );
    }
}

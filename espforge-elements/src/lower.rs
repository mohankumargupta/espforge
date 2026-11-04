//! Lowering: walk `# peripherals`, `# components`, `# devices` and
//! `# services` in rank order, find the registered element for each tag, and
//! build the board.
//!
//! An unknown tag is reported from the schema ∩ registry, so the "known"
//! list in the message cannot drift from what actually exists.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::{Diagnostic, codes};

use crate::element::{Element, LowerCx};
use crate::model::{Board, Bus, Component, GpioDecl, Meta};
use crate::registry;
use crate::schema::schema;

/// Lower the hardware sections onto `meta`, in rank order.
///
/// Diagnostics are collected and returned together; a `Board` exists only when
/// there are none. A bus pin on a declared `<gpio>` stops the walk before the
/// components, in the legacy order the diagnostics corpus pins. Devices are
/// lowered after the components they bind to, and services last: a pass over
/// a failed one below it would only report the same missing reference twice.
pub fn lower(
    meta: Meta,
    peripherals: &[ParsedNode<'_>],
    components: &[ParsedNode<'_>],
    devices: &[ParsedNode<'_>],
    services: &[ParsedNode<'_>],
) -> Result<Board, Vec<Diagnostic>> {
    let mut cx = LowerCx::new();
    cx.set_meta(&meta);
    lower_section("peripherals", peripherals, &mut cx);
    check_bus_gpio_declarations(&mut cx);
    // Legacy stopped at the first bus/gpio conflict before a component could
    // claim the same pin, and the corpus pins that single error. Keep that
    // order: a component is not lowered onto a pin the board already called
    // out. Peripherals are all lowered first, so a `<gpio>` written after the
    // bus that uses its pin still reaches this pass.
    if cx.has_errors() {
        return cx.into_board(meta);
    }
    lower_section("components", components, &mut cx);
    check_spi_cs_gpio_declarations(&mut cx);
    if cx.has_errors() {
        return cx.into_board(meta);
    }
    // Devices bind down to components, so they are lowered after them. A
    // failed component pass stops here too: every device would report the
    // same missing reference.
    lower_section("devices", devices, &mut cx);
    if cx.has_errors() {
        return cx.into_board(meta);
    }
    lower_section("services", services, &mut cx);
    cx.into_board(meta)
}

/// A chip select written as a pin number that collides with a declared
/// `<gpio>`: the gpio is a second user of the pin unless the chip select named
/// that gpio itself. Runs after the components are lowered, because the
/// component records which gpio (if any) its `cs` named.
fn check_spi_cs_gpio_declarations(cx: &mut LowerCx) {
    let gpios: Vec<GpioDecl> = cx.gpios().to_vec();
    let cs_gpios: Vec<String> = cx.cs_gpios().to_vec();
    let mut found: Option<(std::ops::Range<usize>, String)> = None;
    for component in cx.components() {
        let Component::Spi(spi) = component else {
            continue;
        };
        for gpio in &gpios {
            if gpio.pin == spi.cs.pin().0 && !cs_gpios.contains(&gpio.id.raw) {
                found = Some((
                    gpio.span.clone(),
                    format!(
                        "<gpio id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{}",
                        gpio.id.raw, spi.id.raw, gpio.pin
                    ),
                ));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    if let Some((span, label)) = found {
        cx.report(codes::CONFLICT_PIN, span, "pin conflict", label);
    }
}

/// A bus pin that a `<gpio>` declared is a wiring error: the gpio is a second
/// user of the pin, and the diagnostic points at the gpio (as the legacy
/// `check_bus_gpio_declarations` did).
fn check_bus_gpio_declarations(cx: &mut LowerCx) {
    let gpios: Vec<GpioDecl> = cx.gpios().to_vec();
    let mut found: Option<(std::ops::Range<usize>, String)> = None;
    for bus in cx.buses() {
        let (tag, id, pins) = match bus {
            Bus::I2c { id, sda, scl, .. } => ("i2c", id, vec![sda.pin().0, scl.pin().0]),
            Bus::Spi {
                id,
                sclk,
                mosi,
                miso,
                ..
            } => (
                "spi",
                id,
                vec![sclk.pin().0, mosi.pin().0]
                    .into_iter()
                    .chain(miso.iter().map(|pin| pin.pin().0))
                    .collect(),
            ),
            Bus::Uart { id, tx, rx, .. } => ("uart", id, vec![tx.pin().0, rx.pin().0]),
        };
        for gpio in &gpios {
            if pins.contains(&gpio.pin) {
                found = Some((
                    gpio.span.clone(),
                    format!(
                        "<gpio id=\"{}\"> and <{tag} id=\"{}\"> both use GPIO{}",
                        gpio.id.raw, id.raw, gpio.pin
                    ),
                ));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    if let Some((span, label)) = found {
        cx.report(codes::CONFLICT_PIN, span, "pin conflict", label);
    }
}

fn lower_section(section: &str, nodes: &[ParsedNode<'_>], cx: &mut LowerCx) {
    for node in nodes {
        match element_for(section, node.tag_name) {
            Some(element) => {
                let _ = element.lower(node, cx);
            }
            None => {
                let known = known_tags(section);
                let known_list = if known.is_empty() {
                    "(none)".to_string()
                } else {
                    known
                        .iter()
                        .map(|tag| format!("`<{tag}>`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let code = match section {
                    "peripherals" => codes::PERIPHERAL_UNSUPPORTED,
                    "devices" => codes::DEVICE_UNSUPPORTED,
                    "services" => codes::SERVICE_UNSUPPORTED,
                    _ => codes::COMPONENT_UNSUPPORTED,
                };
                cx.report(
                    code,
                    node_span(node),
                    format!(
                        "`<{}>` is not a known element in `# {section}`; known: {known_list}",
                        node.tag_name
                    ),
                    "remove the element, or fix its tag",
                );
            }
        }
    }
}

/// The registered element whose tag matches **and** whose schema section is
/// the one being walked.
fn element_for(section: &str, tag: &str) -> Option<&'static dyn Element> {
    let schema = schema();
    let definition = schema.element(tag)?;
    if definition.section != section {
        return None;
    }
    registry::element(tag)
}

/// The tags that can appear in `section`: schema elements with an
/// implementation, in schema order.
fn known_tags(section: &str) -> Vec<String> {
    let schema = schema();
    schema
        .elements_in(section)
        .filter(|element| registry::element(&element.tag).is_some())
        .map(|element| element.tag.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Lowered;
    use crate::model::{
        Component, Console, Direction, Ident, OwnerKind, Pin, PinUse, Runtime, Service,
    };
    use espf::parser::parse_espf;
    use espforge_diagnostics::Severity;

    const BLINK: &str = "# espforge\n\
        \x20 <project name=\"blink\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n\
        # components\n\
        \x20 <led id=\"red_led\" gpio=\"$gpio18\" />\n\
        # devices\n\
        # services\n";

    const BUTTON: &str = "# espforge\n\
        \x20 <project name=\"button\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <gpio id=\"gpio7\" pin=\"7\" direction=\"output\" />\n\
        \x20 <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n\
        # components\n\
        \x20 <led id=\"red_led\" gpio=\"$gpio7\" active_low=\"false\" />\n\
        \x20 <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n\
        # devices\n\
        # services\n";

    fn meta() -> Meta {
        Meta {
            name: "blink".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        }
    }

    fn lower_src(src: &str) -> Result<Board, Vec<Diagnostic>> {
        lower_src_with(src, meta())
    }

    fn lower_src_with(src: &str, meta: Meta) -> Result<Board, Vec<Diagnostic>> {
        let parsed = parse_espf(src);
        lower(
            meta,
            &parsed.sections["peripherals"],
            &parsed.sections["components"],
            &parsed.sections["devices"],
            &parsed.sections["services"],
        )
    }

    #[test]
    fn blink_lowers_to_one_led_on_pin_18() {
        let board = lower_src(BLINK).expect("blink lowers");
        assert_eq!(board.components.len(), 1);
        let Component::Recipe(led) = &board.components[0] else {
            panic!("blink lowers to a led");
        };
        assert_eq!(led.tag, "led");
        assert_eq!(led.id.raw, "red_led");
        assert_eq!(led.id.rust_name(), "red_led");
        assert_eq!(led.pin.pin(), Pin(18));
        assert_eq!(led.flags, [false]);
        assert_eq!(led.overlay_attr("pin").as_deref(), Some("18"));
        assert_eq!(led.overlay_attr("color"), None);
        assert_eq!(
            board.resources.claim_of(Pin(18)).unwrap().owner.kind,
            OwnerKind::Recipe("led")
        );
    }

    #[test]
    fn a_gpio_without_a_direction_can_still_drive_an_led() {
        let src = BLINK.replace(" direction=\"output\"", "");
        let board = lower_src(&src).expect("no direction is allowed");
        assert_eq!(board.components.len(), 1);
    }

    #[test]
    fn an_input_gpio_cannot_drive_an_led() {
        let src = BLINK.replace("direction=\"output\"", "direction=\"input\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, codes::COMPONENT_WRONG_DIRECTION);
        assert_eq!(errors[0].message, "wrong gpio direction");
        assert!(errors[0].labels[0].text.contains("needs an output pin"));
    }

    #[test]
    fn a_led_pointing_at_a_missing_gpio_is_reported() {
        let src = BLINK.replace("$gpio18", "$gpio99");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UNKNOWN_GPIO);
        assert_eq!(errors[0].message, "unknown gpio");
        assert!(errors[0].labels[0].text.contains("`gpio99`"));
    }

    #[test]
    fn a_bad_gpio_pin_is_e0502_and_a_bad_direction_is_e0503() {
        let errors = lower_src(&BLINK.replace("pin=\"18\"", "pin=\"eighteen\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::GPIO_INVALID_PIN);
        assert_eq!(errors[0].message, "invalid gpio pin");

        let errors = lower_src(&BLINK.replace("direction=\"output\"", "direction=\"sideways\""))
            .unwrap_err();
        assert_eq!(errors[0].code, codes::GPIO_INVALID_DIRECTION);
        assert_eq!(errors[0].message, "invalid gpio direction");
    }

    #[test]
    fn a_led_id_must_become_a_rust_identifier() {
        let errors = lower_src(&BLINK.replace("id=\"red_led\"", "id=\"9red\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_LED);
        assert_eq!(errors[0].message, "invalid led");
        assert!(errors[0].labels[0].text.contains("Rust identifier"));
    }

    #[test]
    fn two_leds_on_one_pin_are_a_conflict() {
        let src = BLINK.replace(
            "  <led id=\"red_led\" gpio=\"$gpio18\" />\n",
            "  <led id=\"red_led\" gpio=\"$gpio18\" />\n  <led id=\"other\" gpio=\"$gpio18\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, codes::CONFLICT_PIN_USED);
        assert_eq!(errors[0].message, "pin already used");
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"red_led\"> and <led id=\"other\"> both use GPIO18"
        );
    }

    #[test]
    fn an_unknown_element_names_what_is_known() {
        let src = BLINK.replace("# peripherals\n", "# peripherals\n  <pwm id=\"pwm0\" />\n");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::PERIPHERAL_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<pwm>` is not a known element in `# peripherals`; known: `<gpio>`, `<i2c>`, `<spi>`, `<uart>`, `<wifi>`"
        );
    }

    #[test]
    fn an_element_in_the_wrong_section_is_not_known_there() {
        let src = BLINK.replace(
            "# components\n",
            "# components\n  <gpio id=\"extra\" pin=\"7\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UNSUPPORTED);
        assert!(
            errors[0]
                .message
                .contains("`<gpio>` is not a known element")
        );
    }

    /// The unknown-tag list for `# components` walks `registry::element`, so
    /// the recipe door must show up in it: a component the schema does not know
    /// is told which components exist.
    #[test]
    fn an_unknown_component_names_the_recipe_elements() {
        let src = BLINK.replace("# components\n", "# components\n  <pwm id=\"pwm0\" />\n");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<pwm>` is not a known element in `# components`; known: `<led>`, `<button>`, `<i2c_component>`, `<spi_component>`, `<uart_component>`, `<wifi_station>`"
        );
    }

    #[test]
    fn button_lowers_beside_an_led_and_claims_its_input_pin() {
        let board = lower_src(BUTTON).expect("button lowers");
        assert_eq!(board.components.len(), 2);

        let Component::Recipe(led) = &board.components[0] else {
            panic!("the first component is the led");
        };
        assert_eq!(led.tag, "led");
        assert_eq!(led.id.raw, "red_led");
        assert_eq!(led.pin.pin(), Pin(7));

        let Component::Recipe(button) = &board.components[1] else {
            panic!("the second component is the button");
        };
        assert_eq!(button.tag, "button");
        assert_eq!(button.id.raw, "button");
        assert_eq!(button.pin.pin(), Pin(9));
        assert_eq!(button.flags, [true]);
        assert_eq!(button.overlay_attr("pin").as_deref(), Some("9"));
        assert_eq!(button.overlay_attr("color"), None);

        let led_claim = board.resources.claim_of(Pin(7)).unwrap();
        assert_eq!(led_claim.owner.kind, OwnerKind::Recipe("led"));
        assert_eq!(led_claim.need, PinUse::Output);
        let button_claim = board.resources.claim_of(Pin(9)).unwrap();
        assert_eq!(button_claim.owner.kind, OwnerKind::Recipe("button"));
        assert_eq!(button_claim.need, PinUse::Input);
    }

    #[test]
    fn a_button_pin_may_leave_its_direction_undeclared() {
        let src = BUTTON.replace(
            "<gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />",
            "<gpio id=\"gpio9\" pin=\"9\" />",
        );
        let board = lower_src(&src).expect("no direction is allowed");
        assert_eq!(board.components.len(), 2);
    }

    #[test]
    fn an_output_gpio_cannot_carry_a_button() {
        let src = BUTTON.replace(
            "<gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />",
            "<gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, codes::COMPONENT_WRONG_DIRECTION);
        assert_eq!(errors[0].message, "wrong gpio direction");
        assert_eq!(
            errors[0].labels[0].text,
            "<button id=\"button\"> needs an input pin, but `gpio9` has direction=\"output\""
        );
    }

    #[test]
    fn pull_up_defaults_to_false_and_must_be_a_flag() {
        let src = BUTTON.replace(" pull_up=\"true\"", "");
        let board = lower_src(&src).expect("pull_up defaults");
        let Component::Recipe(button) = &board.components[1] else {
            panic!("the second component is the button");
        };
        assert_eq!(button.tag, "button");
        assert_eq!(button.flags, [false]);

        let src = BUTTON.replace("pull_up=\"true\"", "pull_up=\"sideways\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_BUTTON);
        assert_eq!(errors[0].message, "invalid button");
        assert_eq!(
            errors[0].labels[0].text,
            "<button id=\"button\"> pull_up=\"sideways\" must be \"true\" or \"false\""
        );
    }

    #[test]
    fn a_button_needs_a_gpio_that_exists_and_an_id_that_becomes_an_identifier() {
        let errors = lower_src(&BUTTON.replace("$gpio9", "$gpio99")).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UNKNOWN_GPIO);
        assert_eq!(errors[0].message, "unknown gpio");
        assert_eq!(
            errors[0].labels[0].text,
            "<button id=\"button\"> refers to `gpio99`, which is not a <gpio> in `# peripherals`"
        );

        let errors = lower_src(&BUTTON.replace("id=\"button\"", "id=\"9btn\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_BUTTON);
        assert_eq!(errors[0].message, "invalid button");
        assert!(errors[0].labels[0].text.contains("Rust identifier"));
    }

    #[test]
    fn two_buttons_on_one_pin_are_e0702() {
        let src = BUTTON.replace(
            "  <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n",
            "  <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n  <button id=\"other\" gpio=\"$gpio9\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, codes::CONFLICT_PIN_USED);
        assert_eq!(errors[0].message, "pin already used");
        assert_eq!(
            errors[0].labels[0].text,
            "<button id=\"button\"> and <button id=\"other\"> both use GPIO9"
        );
    }

    /// The legacy `conflict-led-vs-button` snapshot: a led (output) and a
    /// button (input) on one pin are a direction clash, not a plain conflict.
    #[test]
    fn a_led_and_a_button_on_one_pin_are_a_direction_clash() {
        let src = BUTTON
            .replace(
                "  <led id=\"red_led\" gpio=\"$gpio7\" active_low=\"false\" />\n  <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n",
                "  <led id=\"red_led\" gpio=\"$gpio9\" active_low=\"false\" />\n  <button id=\"btn\" gpio=\"$gpio9\" pull_up=\"true\" />\n",
            )
            // The legacy snapshot leaves the direction undeclared, so the
            // clash is the pin claim's, not a wrong-direction error.
            .replace(
                "<gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />",
                "<gpio id=\"gpio9\" pin=\"9\" />",
            );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, codes::CONFLICT_DIRECTION);
        assert_eq!(errors[0].message, "pin direction clash");
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"red_led\"> and <button id=\"btn\">: one pin cannot be both input and output"
        );
    }

    #[test]
    fn button_is_not_a_peripheral() {
        let src = BUTTON.replace(
            "# peripherals\n",
            "# peripherals\n  <button id=\"extra\" gpio=\"$gpio7\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::PERIPHERAL_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<button>` is not a known element in `# peripherals`; known: `<gpio>`, `<i2c>`, `<spi>`, `<uart>`, `<wifi>`"
        );
    }

    const I2C: &str = "# espforge\n\
        \x20 <project name=\"i2c\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" frequency=\"100kHz\" />\n\
        # components\n\
        \x20 <i2c_component id=\"i2c_tester\" i2c=\"$i2c0\" />\n\
        # devices\n\
        # services\n";

    #[test]
    fn i2c_lowers_to_a_bus_and_one_component() {
        let board = lower_src(I2C).expect("i2c lowers");
        assert_eq!(board.components.len(), 1);
        let Component::I2c(component) = &board.components[0] else {
            panic!("the component is an i2c component");
        };
        assert_eq!(component.id.raw, "i2c_tester");
        let Bus::I2c {
            id,
            peripheral,
            sda,
            scl,
            khz,
        } = &board.resources.buses()[0]
        else {
            panic!("the bus is an i2c bus");
        };
        assert_eq!(id.raw, "i2c0");
        assert_eq!(peripheral, "I2C0");
        assert_eq!((sda.pin().0, scl.pin().0, *khz), (6, 7, 100));
        assert_eq!(component.address, None, "no address is the raw bus handle");
        assert_eq!(
            board.resources.claim_of(Pin(6)).unwrap().need,
            PinUse::Bidirectional
        );
        assert_eq!(
            board.resources.claim_of(Pin(7)).unwrap().need,
            PinUse::Bidirectional
        );
    }

    #[test]
    fn i2c_ids_are_the_three_esp_hal_peripherals() {
        let errors = lower_src(&I2C.replace("id=\"i2c0\"", "id=\"bus\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::I2C_INVALID_ID);
        assert_eq!(errors[0].message, "invalid i2c id");
        assert_eq!(
            errors[0].labels[0].text,
            "<i2c id=\"bus\">: the id must be i2c0, i2c1 or i2c2"
        );
    }

    #[test]
    fn an_i2c_frequency_must_be_khz() {
        let errors =
            lower_src(&I2C.replace("frequency=\"100kHz\"", "frequency=\"fast\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::I2C_INVALID_FREQUENCY);
        assert_eq!(errors[0].message, "invalid i2c frequency");
        assert_eq!(
            errors[0].labels[0].text,
            "<i2c id=\"i2c0\"> frequency=\"fast\" is not a frequency: use e.g. `100kHz`"
        );
    }

    #[test]
    fn i2c_sda_and_scl_on_one_pin_are_i2c_wiring() {
        let errors = lower_src(&I2C.replace("scl=\"7\"", "scl=\"6\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_I2C_WIRING);
        assert_eq!(errors[0].message, "i2c wiring");
        assert_eq!(
            errors[0].labels[0].text,
            "<i2c id=\"i2c0\"> sda and scl cannot both be GPIO6"
        );
    }

    #[test]
    fn a_bus_pin_on_a_declared_gpio_points_at_the_gpio() {
        let src = I2C.replace(
            "  <i2c id=\"i2c0\"",
            "  <gpio id=\"g\" pin=\"6\" />\n  <i2c id=\"i2c0\"",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(errors[0].message, "pin conflict");
        assert_eq!(
            errors[0].labels[0].text,
            "<gpio id=\"g\"> and <i2c id=\"i2c0\"> both use GPIO6"
        );
    }

    #[test]
    fn two_endpoints_share_one_i2c_bus() {
        let src = I2C.replace(
            "  <i2c_component id=\"i2c_tester\" i2c=\"$i2c0\" />\n",
            "  <i2c_component id=\"display\" i2c=\"$i2c0\" address=\"0x3C\" />\n  <i2c_component id=\"sensor\" i2c=\"$i2c0\" address=\"0x62\" />\n",
        );
        let board = lower_src(&src).expect("one bus, two addressed endpoints");
        assert_eq!(board.components.len(), 2);
        let addresses: Vec<Option<u8>> = board
            .components
            .iter()
            .map(|component| component.address())
            .collect();
        assert_eq!(addresses, [Some(0x3C), Some(0x62)]);
    }

    #[test]
    fn two_endpoints_with_one_address_are_a_conflict() {
        let src = I2C.replace(
            "  <i2c_component id=\"i2c_tester\" i2c=\"$i2c0\" />\n",
            "  <i2c_component id=\"display\" i2c=\"$i2c0\" address=\"0x3C\" />\n  <i2c_component id=\"other\" i2c=\"$i2c0\" address=\"0x3C\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_I2C_ADDRESS);
        assert_eq!(errors[0].message, "i2c address conflict");
        assert_eq!(
            errors[0].labels[0].text,
            "<i2c_component id=\"display\"> and <i2c_component id=\"other\"> both use address 0x3C on <i2c id=\"i2c0\">"
        );
    }

    #[test]
    fn one_address_on_two_buses_is_fine() {
        let src = I2C
            .replace(
                "  <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" frequency=\"100kHz\" />\n",
                "  <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" frequency=\"100kHz\" />\n  <i2c id=\"i2c1\" sda=\"2\" scl=\"3\" frequency=\"100kHz\" />\n",
            )
            .replace(
                "  <i2c_component id=\"i2c_tester\" i2c=\"$i2c0\" />\n",
                "  <i2c_component id=\"first\" i2c=\"$i2c0\" address=\"0x3C\" />\n  <i2c_component id=\"second\" i2c=\"$i2c1\" address=\"0x3C\" />\n",
            );
        let board = lower_src(&src).expect("different controllers may share an address");
        assert_eq!(board.components.len(), 2);
    }

    #[test]
    fn reserved_i2c_addresses_are_refused() {
        for address in [0x00u8, 0x07, 0x78, 0x7F] {
            let src = I2C.replace(
                "i2c=\"$i2c0\" />",
                &format!("i2c=\"$i2c0\" address=\"0x{address:02X}\" />"),
            );
            let errors = lower_src(&src).unwrap_err();
            assert_eq!(errors[0].code, codes::COMPONENT_INVALID_I2C);
            assert_eq!(errors[0].message, "invalid i2c_component");
            assert_eq!(
                errors[0].labels[0].text,
                format!(
                    "<i2c_component id=\"i2c_tester\"> address=\"0x{address:02X}\" is reserved by the \
                     I2C specification; usable addresses are 0x08..=0x77"
                )
            );
        }
        // The ends of the usable range lower.
        for address in [0x08u8, 0x77] {
            let src = I2C.replace(
                "i2c=\"$i2c0\" />",
                &format!("i2c=\"$i2c0\" address=\"0x{address:02X}\" />"),
            );
            let board = lower_src(&src).unwrap_or_else(|e| panic!("0x{address:02X}: {e:?}"));
            assert_eq!(board.components[0].address(), Some(address));
        }
    }

    #[test]
    fn an_unaddressed_endpoint_beside_an_addressed_one_lowers() {
        let src = I2C.replace(
            "  <i2c_component id=\"i2c_tester\" i2c=\"$i2c0\" />\n",
            "  <i2c_component id=\"scanner\" i2c=\"$i2c0\" />\n  <i2c_component id=\"sensor\" i2c=\"$i2c0\" address=\"0x62\" />\n",
        );
        let board = lower_src(&src).expect("a raw handle beside an endpoint");
        let addresses: Vec<Option<u8>> = board
            .components
            .iter()
            .map(|component| component.address())
            .collect();
        assert_eq!(addresses, [None, Some(0x62)]);
    }

    #[test]
    fn an_unknown_i2c_reference_is_reported() {
        let errors = lower_src(&I2C.replace("$i2c0", "$i2c9")).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UNKNOWN_BUS);
        assert_eq!(errors[0].message, "unknown i2c bus");
        assert_eq!(
            errors[0].labels[0].text,
            "<i2c_component id=\"i2c_tester\"> refers to `i2c9`, which is not an <i2c> in `# peripherals`"
        );
    }

    #[test]
    fn an_i2c_address_must_parse() {
        let src = I2C.replace("i2c=\"$i2c0\" />", "i2c=\"$i2c0\" address=\"0xZZ\" />");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_I2C);
        assert_eq!(errors[0].message, "invalid i2c_component");
        assert_eq!(
            errors[0].labels[0].text,
            "❌ [i2c_component] 'i2c_tester' invalid hex address '0xZZ'"
        );
    }

    const SPI: &str = "# espforge\n\
        \x20 <project name=\"spi\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" mode=\"0\" frequency=\"100kHz\" />\n\
        # components\n\
        \x20 <spi_component id=\"spi_tester\" spi=\"$spi2\" cs=\"9\" />\n\
        # devices\n\
        # services\n";

    #[test]
    fn spi_lowers_to_a_full_duplex_bus_and_a_device() {
        let board = lower_src(SPI).expect("spi lowers");
        assert_eq!(board.components.len(), 1);
        let Bus::Spi {
            peripheral,
            sclk,
            mosi,
            miso,
            khz,
            mode,
            ..
        } = &board.resources.buses()[0]
        else {
            panic!("the bus is an spi bus");
        };
        assert_eq!(peripheral, "SPI2");
        assert_eq!(
            (sclk.pin().0, mosi.pin().0, miso.as_ref().unwrap().pin().0),
            (8, 7, 6)
        );
        assert_eq!((*khz, *mode), (100, 0));
        assert_eq!(
            board.resources.claim_of(Pin(6)).unwrap().need,
            PinUse::Bidirectional
        );
    }

    #[test]
    fn only_spi2_is_a_valid_spi_id() {
        let errors = lower_src(&SPI.replace("id=\"spi2\"", "id=\"spi3\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::SPI_INVALID_ID);
        assert_eq!(errors[0].message, "invalid spi id");
        assert_eq!(
            errors[0].labels[0].text,
            "<spi id=\"spi3\">: the id must be spi2"
        );
    }

    #[test]
    fn a_missing_miso_is_a_write_only_bus() {
        let board = lower_src(&SPI.replace(" miso=\"6\"", "")).expect("write-only bus");
        let Bus::Spi { miso, .. } = &board.resources.buses()[0] else {
            panic!("the bus is an spi bus");
        };
        assert!(miso.is_none());
    }

    #[test]
    fn an_out_of_range_spi_mode_is_refused() {
        let errors = lower_src(&SPI.replace("mode=\"0\"", "mode=\"7\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::SPI_INVALID_MODE);
        assert_eq!(errors[0].message, "invalid spi mode");
        assert_eq!(
            errors[0].labels[0].text,
            "<spi id=\"spi2\"> mode=\"7\" must be 0, 1, 2 or 3"
        );
    }

    #[test]
    fn spi_sclk_and_mosi_on_one_pin_are_spi_wiring() {
        let errors = lower_src(&SPI.replace("mosi=\"7\"", "mosi=\"8\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_SPI_WIRING);
        assert_eq!(errors[0].message, "spi wiring");
        assert_eq!(
            errors[0].labels[0].text,
            "<spi id=\"spi2\"> sclk and mosi cannot both be GPIO8"
        );
    }

    #[test]
    fn a_chip_select_reference_must_exist() {
        let errors = lower_src(&SPI.replace("cs=\"9\"", "cs=\"$gpioX\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_CS);
        assert_eq!(errors[0].message, "invalid spi_component");
        assert_eq!(
            errors[0].labels[0].text,
            "❌ [spi_component] 'spi_tester' cs references undefined gpio 'gpioX'"
        );
    }

    #[test]
    fn a_chip_select_pin_on_a_declared_gpio_points_at_the_gpio() {
        let src = SPI.replace(
            "  <spi id=\"spi2\"",
            "  <gpio id=\"g\" pin=\"9\" />\n  <spi id=\"spi2\"",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(
            errors[0].labels[0].text,
            "<gpio id=\"g\"> and <spi_component id=\"spi_tester\"> both use GPIO9"
        );
    }

    #[test]
    fn a_chip_select_naming_its_own_gpio_is_not_a_conflict() {
        let src = SPI.replace("cs=\"9\"", "cs=\"$gpio9\"").replace(
            "  <spi id=\"spi2\"",
            "  <gpio id=\"gpio9\" pin=\"9\" />\n  <spi id=\"spi2\"",
        );
        let board = lower_src(&src).expect("cs names its own gpio");
        assert_eq!(board.components.len(), 1);
    }

    #[test]
    fn two_spi_devices_on_one_bus_are_allowed() {
        let src = SPI.replace(
            "  <spi_component id=\"spi_tester\" spi=\"$spi2\" cs=\"9\" />\n",
            "  <spi_component id=\"spi_tester\" spi=\"$spi2\" cs=\"9\" />\n  <spi_component id=\"other\" spi=\"$spi2\" cs=\"10\" />\n",
        );
        let board = lower_src(&src).expect("one bus, two devices");
        assert_eq!(board.components.len(), 2);
    }

    const UART: &str = "# espforge\n\
        \x20 <project name=\"uart\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" />\n\
        # components\n\
        \x20 <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n\
        # devices\n\
        # services\n";

    #[test]
    fn uart_lowers_to_a_port_and_its_framing() {
        let board = lower_src(UART).expect("uart lowers");
        let Bus::Uart {
            peripheral,
            tx,
            rx,
            used_by,
            ..
        } = &board.resources.buses()[0]
        else {
            panic!("the bus is a uart bus");
        };
        assert_eq!(peripheral, "UART1");
        assert_eq!((tx.pin().0, rx.pin().0), (6, 5));
        assert_eq!(used_by.as_ref().map(|id| id.raw.as_str()), Some("serial"));
        assert_eq!(
            board.resources.claim_of(Pin(6)).unwrap().need,
            PinUse::Unspecified
        );
        let Component::Uart(component) = &board.components[0] else {
            panic!("the component is a uart component");
        };
        assert_eq!(
            (component.baud, component.framing.parity),
            (9600, crate::model::Parity::None)
        );
        assert_eq!(component.framing.stop_bits, crate::model::StopBits::One);
    }

    #[test]
    fn uart_tx_and_rx_on_one_pin_are_uart_wiring() {
        let errors = lower_src(&UART.replace("rx=\"5\"", "rx=\"6\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_UART_WIRING);
        assert_eq!(errors[0].message, "uart wiring");
        assert_eq!(
            errors[0].labels[0].text,
            "<uart id=\"uart1\"> tx and rx cannot both be GPIO6"
        );
    }

    #[test]
    fn two_uarts_on_one_peripheral_are_a_peripheral_clash() {
        let src = UART.replace(
            "  <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" />\n",
            "  <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" />\n  <uart id=\"other\" uart=\"UART1\" tx=\"7\" rx=\"8\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PERIPHERAL);
        assert_eq!(errors[0].message, "peripheral clash");
        assert_eq!(
            errors[0].labels[0].text,
            "<uart id=\"uart1\"> and <uart id=\"other\"> both use UART1"
        );
    }

    #[test]
    fn an_explicit_uart_attribute_names_the_peripheral() {
        let src = UART
            .replace("id=\"uart1\"", "id=\"gps\"")
            .replace("$uart1", "$gps");
        let board = lower_src(&src).expect("gps names UART1");
        let Bus::Uart { peripheral, id, .. } = &board.resources.buses()[0] else {
            panic!("the bus is a uart bus");
        };
        assert_eq!((id.raw.as_str(), peripheral.as_str()), ("gps", "UART1"));
    }

    #[test]
    fn an_uart_id_must_be_one_of_the_four_ports() {
        let src = UART
            .replace("id=\"uart1\" uart=\"UART1\"", "id=\"uart4\"")
            .replace("$uart1", "$uart4");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::UART_INVALID_ID);
        assert_eq!(errors[0].message, "invalid uart id");
        assert_eq!(
            errors[0].labels[0].text,
            "<uart id=\"uart4\">: the id must be uart0, uart1, uart2 or uart3 (or give uart=\"UART1\" explicitly)"
        );
    }

    #[test]
    fn a_zero_baud_rate_is_refused() {
        let errors = lower_src(&UART.replace("baud_rate=\"9600\"", "baud_rate=\"0\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_BAUD);
        assert_eq!(errors[0].message, "invalid baud_rate");
        assert_eq!(
            errors[0].labels[0].text,
            "<uart_component id=\"serial\"> baud_rate=\"0\" must be greater than 0"
        );
    }

    #[test]
    fn one_uart_port_takes_exactly_one_component() {
        let src = UART.replace(
            "  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n",
            "  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n  <uart_component id=\"other\" uart=\"$uart1\" baud_rate=\"115200\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UART_USED);
        assert_eq!(errors[0].message, "uart already used");
        assert_eq!(
            errors[0].labels[0].text,
            "<uart_component id=\"serial\"> and <uart_component id=\"other\"> both use <uart id=\"uart1\">"
        );
    }

    #[test]
    fn the_gpio_direction_is_carried_as_a_domain_value() {
        let src = BLINK.replace("direction=\"output\"", "direction=\"sideways\"");
        let parsed = parse_espf(&src);
        let mut cx = LowerCx::new();
        lower_section("peripherals", &parsed.sections["peripherals"], &mut cx);
        assert!(cx.gpio("gpio18").is_none(), "nothing is recorded on error");

        let parsed = parse_espf(BLINK);
        let mut cx = LowerCx::new();
        lower_section("peripherals", &parsed.sections["peripherals"], &mut cx);
        assert_eq!(
            cx.gpio("gpio18").unwrap().direction,
            Some(Direction::Output)
        );
    }

    /// A station on an esp32c3 radio: the base every service test replaces a
    /// line of.
    const WIFI_STATION: &str = "# espforge\n\
        \x20 <project name=\"wifi\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"embassy\" />\n\
        # peripherals\n\
        \x20 <wifi id=\"radio\" />\n\
        # components\n\
        \x20 <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n\
        # devices\n\
        # services\n";

    fn embassy_meta() -> Meta {
        Meta {
            runtime: Runtime::Embassy,
            ..meta()
        }
    }

    /// Put one service on [`WIFI_STATION`]'s `# services` line.
    fn with_service(service: &str) -> String {
        WIFI_STATION.replace("# services\n", &format!("# services\n  {service}\n"))
    }

    #[test]
    fn a_station_lowers_under_its_radio() {
        let board = lower_src_with(WIFI_STATION, embassy_meta()).expect("the station lowers");
        let Component::WifiStation(station) = &board.components[0] else {
            panic!("the component is a wifi station");
        };
        assert_eq!(station.id.raw, "home");
        assert_eq!(station.radio.raw, "radio");
        assert_eq!(station.ssid, "Wokwi-GUEST");
        assert_eq!(
            station.password,
            crate::components::wifi_station::Password::Open
        );
    }

    #[test]
    fn a_station_must_name_a_radio_that_exists() {
        let src = WIFI_STATION.replace("$radio", "$nope");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_UNKNOWN_WIFI);
        assert_eq!(errors[0].message, "unknown wifi radio");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi_station id=\"home\"> refers to `nope`, which is not a <wifi> in `# peripherals`"
        );
    }

    #[test]
    fn one_radio_takes_exactly_one_station() {
        let src = WIFI_STATION.replace(
            "  <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n",
            "  <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n\
             \x20 <wifi_station id=\"other\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n",
        );
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_WIFI_RADIO_USED);
        assert_eq!(errors[0].message, "wifi radio already used");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi_station id=\"home\"> and <wifi_station id=\"other\"> both use <wifi id=\"radio\">"
        );
    }

    #[test]
    fn a_station_without_a_wifi_attribute_names_the_missing_one() {
        let src = WIFI_STATION.replace(" wifi=\"$radio\"", "");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_WIFI);
        assert_eq!(errors[0].message, "invalid wifi_station");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi_station id=\"home\"> is missing `wifi`"
        );
    }

    #[test]
    fn an_ssid_must_be_one_to_32_bytes() {
        let src = WIFI_STATION.replace("ssid=\"Wokwi-GUEST\"", "ssid=\"\"");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_SSID);
        assert_eq!(errors[0].message, "invalid ssid");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi_station id=\"home\"> ssid must be 1 to 32 bytes, but \"\" is 0"
        );
    }

    #[test]
    fn a_wpa2_station_needs_a_password_of_8_to_63_characters() {
        let src = WIFI_STATION.replace(" auth=\"open\"", "");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_PASSWORD);
        assert_eq!(errors[0].message, "missing password");

        let src = WIFI_STATION.replace("auth=\"open\"", "password=\"short\"");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_PASSWORD);
        assert_eq!(errors[0].message, "invalid password");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi_station id=\"home\"> password must be 8 to 63 characters, but this one is 5"
        );
    }

    #[test]
    fn an_open_network_carries_no_password() {
        let src = WIFI_STATION.replace("auth=\"open\"", "auth=\"open\" password=\"secret12\"");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_OPEN_WITH_PASSWORD);
        assert_eq!(errors[0].message, "password given for an open network");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi_station id=\"home\"> auth=\"open\" cannot carry a password"
        );
    }

    #[test]
    fn an_env_password_names_a_variable_that_could_exist() {
        let src = WIFI_STATION.replace("auth=\"open\"", "password=\"env:1bad\"");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_INVALID_ENV);
        assert_eq!(errors[0].message, "invalid environment variable name");

        let src = WIFI_STATION.replace("auth=\"open\"", "password=\"env:WIFI_PASSWORD\"");
        let board = lower_src_with(&src, embassy_meta()).expect("a good env name lowers");
        let Component::WifiStation(station) = &board.components[0] else {
            panic!("the component is a wifi station");
        };
        assert_eq!(
            station.password,
            crate::components::wifi_station::Password::Env("WIFI_PASSWORD".into())
        );
    }

    #[test]
    fn a_wifi_radio_only_lowers_on_a_chip_that_has_one() {
        let h2 = Meta {
            chip: "esp32h2".into(),
            ..embassy_meta()
        };
        let errors = lower_src_with(WIFI_STATION, h2).unwrap_err();
        assert_eq!(errors[0].code, codes::WIFI_NO_RADIO);
        assert_eq!(errors[0].message, "no wifi radio");
        assert_eq!(
            errors[0].labels[0].text,
            "<wifi id=\"radio\">: esp32h2 has no Wi-Fi radio"
        );
    }

    /// The config expression one field of `service` holds.
    fn field_of<'a>(service: &'a Service, name: &str) -> &'a str {
        service
            .config_fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, expr)| expr.as_str())
            .unwrap_or_else(|| panic!("`{name}` is not a config field of <{}>", service.tag))
    }

    #[test]
    fn an_http_client_lowers_over_the_stations_stack() {
        let src = with_service("<http_client id=\"client\" network=\"$home\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("the client lowers");
        let client = &board.services[0];
        assert_eq!(client.id.raw, "client");
        assert_eq!(client.network.raw, "home");
        assert_eq!(client.tag, "http_client");
        assert_eq!(client.feature, "http-client");
        assert_eq!(client.ty, "espforge_runtime::HttpClient");
        assert_eq!(client.new, "espforge_runtime::HttpClient::new");
        assert_eq!(
            client.config.as_deref(),
            Some("espforge_runtime::HttpClientConfig")
        );
        assert_eq!(field_of(client, "rx_buffer_size"), "2048u32 as usize");
        assert_eq!(field_of(client, "tx_buffer_size"), "1024u32 as usize");
        assert_eq!(field_of(client, "timeout_ms"), "10000u32 as u64");
        assert!(client.extra_args.is_empty());
        assert!(board.warnings.is_empty(), "no warning without insecure");
    }

    #[test]
    fn a_service_network_must_be_a_component_that_has_a_stack() {
        let src = with_service("<http_client id=\"client\" network=\"$nope\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNKNOWN_NETWORK);
        assert_eq!(errors[0].message, "unknown network interface");
        assert_eq!(
            errors[0].labels[0].text,
            "<http_client id=\"client\"> refers to `nope`, which is not a network component in `# components`"
        );
    }

    #[test]
    fn a_service_requires_the_embassy_runtime() {
        let src = with_service("<http_client id=\"client\" network=\"$home\" />");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_REQUIRES_EMBASSY);
        assert_eq!(errors[0].message, "http_client requires embassy");
        assert_eq!(
            errors[0].labels[0].text,
            "<http_client id=\"client\"> needs the embassy runtime; change <runtime type=\"..\" /> to embassy"
        );
    }

    #[test]
    fn a_buffer_below_the_protocol_floor_is_refused() {
        let src =
            with_service("<http_client id=\"client\" network=\"$home\" rx_buffer_size=\"256\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_BUFFER_TOO_SMALL);
        assert_eq!(errors[0].message, "buffer too small");
        assert_eq!(
            errors[0].labels[0].text,
            "<http_client id=\"client\"> rx_buffer_size=\"256\" is below the minimum of 512 bytes"
        );

        // TLS records need more room than plain HTTP headers do.
        let src = with_service(
            "<https_client id=\"secure\" network=\"$home\" rx_buffer_size=\"1024\" />",
        );
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_BUFFER_TOO_SMALL);
        assert_eq!(
            errors[0].labels[0].text,
            "<https_client id=\"secure\"> rx_buffer_size=\"1024\" is below the minimum of 4096 bytes"
        );
    }

    #[test]
    fn an_insecure_https_client_warns_and_still_lowers() {
        let src =
            with_service("<https_client id=\"secure\" network=\"$home\" insecure=\"true\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("a warning is not an error");
        assert_eq!(board.warnings.len(), 1);
        assert_eq!(board.warnings[0].severity, Severity::Warning);
        assert_eq!(board.warnings[0].code, codes::SERVICE_INSECURE_WARNING);
        assert_eq!(
            board.warnings[0].message,
            "insecure TLS verification enabled"
        );
        assert_eq!(
            board.warnings[0].labels[0].text,
            "server certificates will not be validated"
        );
        let client = &board.services[0];
        assert_eq!(client.extra_args, ["true"]);
        assert_eq!(client.heap_bytes, Some(98304));
        assert_eq!(field_of(client, "rx_buffer_size"), "4096u32 as usize");
        assert_eq!(field_of(client, "timeout_ms"), "15000u32 as u64");
    }

    #[test]
    fn an_unknown_service_tag_names_the_services() {
        let src = with_service("<ntp_client id=\"clock\" network=\"$home\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<ntp_client>` is not a known element in `# services`; known: `<http_client>`, `<https_client>`, `<ws_client>`, `<wss_client>`, `<ntp>`"
        );
    }

    #[test]
    fn a_service_id_must_become_a_rust_identifier() {
        let src = with_service("<http_client id=\"2fast\" network=\"$home\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_INVALID);
        assert_eq!(errors[0].message, "invalid http_client");
    }

    #[test]
    fn a_ws_client_lowers_over_the_stations_stack() {
        let src = with_service("<ws_client id=\"ws\" network=\"$home\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("the ws client lowers");
        let client = &board.services[0];
        assert_eq!(client.id.raw, "ws");
        assert_eq!(client.network.raw, "home");
        assert_eq!(client.feature, "ws-client");
        assert_eq!(field_of(client, "rx_buffer_size"), "2048u32 as usize");
        assert_eq!(field_of(client, "tx_buffer_size"), "1024u32 as usize");
        assert_eq!(field_of(client, "timeout_ms"), "10000u32 as u64");
        assert_eq!(board.service_features(), ["ws-client"]);
        assert_eq!(board.max_service_heap(), None);
    }

    #[test]
    fn a_ws_client_requires_the_embassy_runtime() {
        let src = with_service("<ws_client id=\"ws\" network=\"$home\" />");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_REQUIRES_EMBASSY);
        assert_eq!(errors[0].message, "ws_client requires embassy");
        assert_eq!(
            errors[0].labels[0].text,
            "<ws_client id=\"ws\"> needs the embassy runtime; change <runtime type=\"..\" /> to embassy"
        );
    }

    #[test]
    fn a_ws_client_network_must_be_a_component_that_has_a_stack() {
        let src = with_service("<ws_client id=\"ws\" network=\"$nope\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNKNOWN_NETWORK);
        assert_eq!(
            errors[0].labels[0].text,
            "<ws_client id=\"ws\"> refers to `nope`, which is not a network component in `# components`"
        );
    }

    #[test]
    fn an_insecure_wss_client_warns_and_still_lowers() {
        let src =
            with_service("<wss_client id=\"secure_ws\" network=\"$home\" insecure=\"true\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("a warning is not an error");
        assert_eq!(board.warnings.len(), 1);
        assert_eq!(board.warnings[0].severity, Severity::Warning);
        assert_eq!(board.warnings[0].code, codes::SERVICE_INSECURE_WARNING);
        let client = &board.services[0];
        assert_eq!(client.extra_args, ["true"]);
        assert_eq!(client.heap_bytes, Some(98304));
        assert_eq!(field_of(client, "rx_buffer_size"), "4096u32 as usize");
        assert_eq!(board.max_service_heap(), Some(98304));
    }

    #[test]
    fn an_ntp_client_lowers_over_the_stations_stack() {
        let src = with_service("<ntp id=\"clock\" network=\"$home\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("the ntp client lowers");
        let client = &board.services[0];
        assert_eq!(client.id.raw, "clock");
        assert_eq!(client.network.raw, "home");
        assert_eq!(client.tag, "ntp");
        assert_eq!(client.feature, "ntp");
        assert_eq!(client.ty, "espforge_runtime::NtpClient");
        assert_eq!(field_of(client, "server"), "\"pool.ntp.org\"");
        assert_eq!(field_of(client, "port"), "123u32 as u16");
        assert_eq!(field_of(client, "timeout_ms"), "5000u32 as u64");
        assert_eq!(board.service_features(), ["ntp"]);
    }

    #[test]
    fn an_ntp_client_takes_a_custom_server_and_timeout() {
        let src = with_service(
            "<ntp id=\"clock\" network=\"$home\" server=\"time.google.com\" port=\"1123\" timeout_ms=\"3000\" />",
        );
        let board = lower_src_with(&src, embassy_meta()).expect("the ntp client lowers");
        let client = &board.services[0];
        assert_eq!(field_of(client, "server"), "\"time.google.com\"");
        assert_eq!(field_of(client, "port"), "1123u32 as u16");
        assert_eq!(field_of(client, "timeout_ms"), "3000u32 as u64");
    }

    #[test]
    fn an_ntp_client_requires_the_embassy_runtime() {
        let src = with_service("<ntp id=\"clock\" network=\"$home\" />");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_REQUIRES_EMBASSY);
        assert_eq!(errors[0].message, "ntp requires embassy");
        assert_eq!(
            errors[0].labels[0].text,
            "<ntp id=\"clock\"> needs the embassy runtime; change <runtime type=\"..\" /> to embassy"
        );
    }

    #[test]
    fn an_ntp_client_network_must_be_a_component_that_has_a_stack() {
        let src = with_service("<ntp id=\"clock\" network=\"$nope\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNKNOWN_NETWORK);
        assert_eq!(
            errors[0].labels[0].text,
            "<ntp id=\"clock\"> refers to `nope`, which is not a network component in `# components`"
        );
    }

    #[test]
    fn an_ntp_port_outside_the_udp_range_is_refused() {
        let src = with_service("<ntp id=\"clock\" network=\"$home\" port=\"0\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_INVALID);
        assert_eq!(errors[0].message, "invalid ntp port");
        assert_eq!(
            errors[0].labels[0].text,
            "<ntp id=\"clock\"> port=\"0\" must be in 1..=65535"
        );

        let src = with_service("<ntp id=\"clock\" network=\"$home\" port=\"65536\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_INVALID);
        assert_eq!(errors[0].message, "invalid ntp port");
    }

    #[test]
    fn an_ntp_server_cannot_be_empty() {
        let src = with_service("<ntp id=\"clock\" network=\"$home\" server=\"\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_INVALID);
        assert_eq!(errors[0].message, "invalid ntp server");
        assert_eq!(
            errors[0].labels[0].text,
            "<ntp id=\"clock\"> server=\"\" must not be empty"
        );
    }

    #[test]
    fn a_websocket_buffer_below_its_floor_is_refused() {
        let src = with_service("<ws_client id=\"ws\" network=\"$home\" rx_buffer_size=\"256\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_BUFFER_TOO_SMALL);
        assert_eq!(
            errors[0].labels[0].text,
            "<ws_client id=\"ws\"> rx_buffer_size=\"256\" is below the minimum of 512 bytes"
        );

        // TLS records need more room than plain WebSocket frames do.
        let src = with_service(
            "<wss_client id=\"secure_ws\" network=\"$home\" rx_buffer_size=\"1024\" />",
        );
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_BUFFER_TOO_SMALL);
        assert_eq!(
            errors[0].labels[0].text,
            "<wss_client id=\"secure_ws\"> rx_buffer_size=\"1024\" is below the minimum of 4096 bytes"
        );
    }

    const OLED: &str = "# espforge\n\
        \x20 <project name=\"oled\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n\
        # components\n\
        \x20 <i2c_component id=\"oled_bus\" i2c=\"$i2c0\" address=\"0x3C\" />\n\
        # devices\n\
        \x20 <ssd1306 id=\"oled\" i2c_component=\"$oled_bus\" size=\"128x32\" rotation=\"180\" />\n\
        # services\n";

    #[test]
    fn an_ssd1306_lowers_onto_its_i2c_component() {
        let board = lower_src(OLED).expect("the oled lowers");
        assert_eq!(board.devices.len(), 1);
        let oled = &board.devices[0];
        assert_eq!(oled.tag, "ssd1306");
        assert_eq!(oled.id.raw, "oled");
        assert_eq!(oled.bus_component.raw, "oled_bus");
        // The recipe's slots are filled from the values as written.
        assert_eq!(
            oled.ty,
            "espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x32>"
        );
        assert_eq!(
            oled.args,
            [
                "espforge_drivers::Oled128x32",
                "espforge_drivers::DisplayRotation::Rotate180"
            ]
        );
        assert_eq!(oled.feature, "ssd1306");
        assert!(!oled.takes_bus);
        assert_eq!(board.driver_features(), ["ssd1306"]);
        assert_eq!(board.driver_deps(), ["embedded-graphics"]);
    }

    /// A display on the wrong component kind is refused: the recipe's bus
    /// attribute names the tag it accepts (`to=` on the schema attribute), so
    /// a typo cannot emit code that does not compile.
    #[test]
    fn an_ssd1306_on_an_spi_component_is_refused() {
        let src = OLED
            .replace(
                "  <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n",
                "  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" />\n",
            )
            .replace(
                "<i2c_component id=\"oled_bus\" i2c=\"$i2c0\" address=\"0x3C\" />",
                "<spi_component id=\"oled_bus\" spi=\"$spi2\" cs=\"9\" />",
            );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNKNOWN_COMPONENT);
        assert_eq!(errors[0].message, "unknown bus component");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> refers to `oled_bus`, which is not an <i2c_component> in `# components`"
        );
    }

    /// A value the schema does not allow is refused, quoting what it does
    /// allow: the recipe maps spellings, so no unmapped value reaches emission.
    #[test]
    fn a_rotation_outside_the_choices_is_refused() {
        let errors = lower_src(&OLED.replace("rotation=\"180\"", "rotation=\"45\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "invalid ssd1306");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> rotation=\"45\" must be one of 0, 90, 180, 270"
        );
    }

    /// The sensor is a device with no auxiliary pin and no dependency of its
    /// own: its project must not pull the graphics crate the displays need.
    #[test]
    fn a_scd4x_lowers_onto_its_i2c_component() {
        let src = OLED
            .replace(
                "<ssd1306 id=\"oled\" i2c_component=\"$oled_bus\" size=\"128x32\" rotation=\"180\" />",
                "<scd4x id=\"air\" i2c_component=\"$oled_bus\" />",
            )
            .replace("address=\"0x3C\"", "address=\"0x62\"");
        let board = lower_src(&src).expect("the sensor lowers");
        let air = &board.devices[0];
        assert_eq!(air.tag, "scd4x");
        assert_eq!(air.id.raw, "air");
        assert_eq!(air.ty, "espforge_drivers::Scd4x");
        assert_eq!(air.new, "espforge_drivers::Scd4x::new");
        assert!(air.args.is_empty());
        assert!(air.pins.is_empty());
        assert_eq!(board.driver_features(), ["scd4x"]);
        assert!(board.driver_deps().is_empty());
    }

    /// A display and a sensor on one controller: the multi-drop case the
    /// schema did not allow before, both lowering onto their own addresses.
    #[test]
    fn a_display_and_a_sensor_share_one_bus() {
        let src = OLED.replace(
            "  <i2c_component id=\"oled_bus\" i2c=\"$i2c0\" address=\"0x3C\" />\n",
            "  <i2c_component id=\"oled_bus\" i2c=\"$i2c0\" address=\"0x3C\" />\n  <i2c_component id=\"sensor_bus\" i2c=\"$i2c0\" address=\"0x62\" />\n",
        )
        .replace(
            "# services",
            "  <scd4x id=\"air\" i2c_component=\"$sensor_bus\" />\n# services",
        );
        let board = lower_src(&src).expect("one bus, two devices");
        assert_eq!(board.devices.len(), 2);
        assert_eq!(board.driver_features(), ["ssd1306", "scd4x"]);
        let buses: Vec<&str> = board
            .components
            .iter()
            .map(|component| component.id().raw.as_str())
            .collect();
        assert_eq!(buses, ["oled_bus", "sensor_bus"]);
    }

    /// A driver takes an `I2cDevice`, so its component must declare an
    /// address: a bus-only component would emit code that does not compile.
    #[test]
    fn a_display_needs_an_addressed_component() {
        let src = OLED.replace(" address=\"0x3C\"", "");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "display needs an i2c address");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> needs <i2c_component id=\"oled_bus\"> to declare an `address`; it is a bus-only component"
        );
    }

    /// A fixed-address part names its address when the component has none.
    #[test]
    fn a_sensor_needs_an_addressed_component() {
        let src = OLED
            .replace(
                "<ssd1306 id=\"oled\" i2c_component=\"$oled_bus\" size=\"128x32\" rotation=\"180\" />",
                "<scd4x id=\"air\" i2c_component=\"$oled_bus\" />",
            )
            .replace(" address=\"0x3C\"", "");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "sensor needs an i2c address");
        assert_eq!(
            errors[0].labels[0].text,
            "<scd4x id=\"air\"> needs <i2c_component id=\"oled_bus\"> to declare an `address` (this part is at 0x62); it is a bus-only component"
        );
    }

    /// A component on the wrong address for a fixed-address part is a wiring
    /// error, and only lowering can see it.
    #[test]
    fn a_sensor_on_another_address_is_refused() {
        let src = OLED
            .replace(
                "<ssd1306 id=\"oled\" i2c_component=\"$oled_bus\" size=\"128x32\" rotation=\"180\" />",
                "<scd4x id=\"air\" i2c_component=\"$oled_bus\" />",
            )
            .replace("address=\"0x3C\"", "address=\"0x48\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "sensor address mismatch");
        assert_eq!(
            errors[0].labels[0].text,
            "<scd4x id=\"air\"> is fixed at address 0x62, but <i2c_component id=\"oled_bus\"> declares 0x48"
        );
    }

    /// The display picks any address: 0x3D is a common alternate strap.
    #[test]
    fn a_display_on_another_address_lowers() {
        let board = lower_src(&OLED.replace("address=\"0x3C\"", "address=\"0x3D\""))
            .expect("the panel address is the project's to choose");
        assert_eq!(board.devices.len(), 1);
    }

    #[test]
    fn an_ssd1306_reference_must_name_an_i2c_component() {
        let errors = lower_src(&OLED.replace("$oled_bus", "$nope")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNKNOWN_COMPONENT);
        assert_eq!(errors[0].message, "unknown bus component");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> refers to `nope`, which is not an <i2c_component> in `# components`"
        );
    }

    #[test]
    fn an_ssd1306_id_must_become_a_rust_identifier() {
        let errors = lower_src(&OLED.replace("id=\"oled\"", "id=\"9led\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID_ID);
        assert_eq!(errors[0].message, "invalid device id");
        assert!(errors[0].labels[0].text.contains("valid Rust identifier"));
    }

    #[test]
    fn a_display_requires_the_blocking_runtime() {
        let errors = lower_src_with(OLED, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_REQUIRES_BLOCKING);
        assert_eq!(errors[0].message, "display requires blocking");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> needs the blocking runtime; change <runtime type=\"..\" /> to blocking"
        );
    }

    #[test]
    fn an_unknown_device_tag_names_the_displays() {
        let src = OLED.replace("<ssd1306 id=\"oled\"", "<st7789 id=\"lcd\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<st7789>` is not a known element in `# devices`; known: `<ssd1306>`, `<ili9341>`, `<scd4x>`"
        );
    }

    const TFT: &str = "# espforge\n\
        \x20 <project name=\"tft\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" mode=\"0\" />\n\
        \x20 <gpio id=\"gpio4\" pin=\"4\" direction=\"output\" />\n\
        \x20 <gpio id=\"gpio5\" pin=\"5\" direction=\"output\" />\n\
        \x20 <gpio id=\"gpio6\" pin=\"6\" direction=\"output\" />\n\
        # components\n\
        \x20 <spi_component id=\"tft_bus\" spi=\"$spi2\" cs=\"9\" />\n\
        # devices\n\
        \x20 <ili9341 id=\"tft\" spi_component=\"$tft_bus\" dc=\"$gpio4\" reset=\"$gpio5\" backlight=\"$gpio6\" rotation=\"90\" />\n\
        # services\n";

    #[test]
    fn an_ili9341_lowers_and_claims_its_control_pins() {
        let board = lower_src(TFT).expect("the tft lowers");
        assert_eq!(board.devices.len(), 1);
        let tft = &board.devices[0];
        assert_eq!(tft.tag, "ili9341");
        assert_eq!(tft.id.raw, "tft");
        assert_eq!(tft.bus_component.raw, "tft_bus");
        let pins: Vec<(&str, u8)> = tft
            .pins
            .iter()
            .map(|(arg, pin)| (arg.as_str(), pin.pin().0))
            .collect();
        assert_eq!(pins, [("dc", 4), ("reset", 5), ("backlight", 6)]);
        for pin in [4, 5, 6] {
            let claim = board.resources.claim_of(Pin(pin)).unwrap();
            assert_eq!(claim.owner.kind, OwnerKind::Device("ili9341"));
            assert_eq!(claim.need, PinUse::Output);
        }
        assert!(tft.takes_bus);
        // The control lines are the recipe's pin templates, filled in.
        assert_eq!(
            tft.args[0],
            "esp_hal::gpio::Output::new(unsafe { registry.peripherals.GPIO4.clone_unchecked() }, esp_hal::gpio::Level::Low, esp_hal::gpio::OutputConfig::default())"
        );
        // An optional pin is an `Option` the driver takes.
        assert!(tft.args[2].starts_with("Some("), "{}", tft.args[2]);
        assert_eq!(tft.args[3], "espforge_drivers::Orientation::Landscape");
        assert_eq!(tft.args[4], "espforge_drivers::ColorOrder::Bgr");
        assert!(board.takes_component(&Ident::new("tft_bus", 0..0)));
        assert_eq!(board.driver_features(), ["ili9341"]);
    }

    #[test]
    fn a_tft_control_pin_must_be_a_declared_output_gpio() {
        let errors = lower_src(&TFT.replace("dc=\"$gpio4\"", "dc=\"$gpio9\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNKNOWN_GPIO);
        assert_eq!(errors[0].message, "unknown gpio");
        assert_eq!(
            errors[0].labels[0].text,
            "<ili9341 id=\"tft\"> refers to `gpio9`, which is not a <gpio> in `# peripherals`"
        );

        let input = TFT.replace(
            "<gpio id=\"gpio4\" pin=\"4\" direction=\"output\" />",
            "<gpio id=\"gpio4\" pin=\"4\" direction=\"input\" />",
        );
        let errors = lower_src(&input).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_WRONG_DIRECTION);
        assert_eq!(errors[0].message, "wrong gpio direction");
        assert_eq!(
            errors[0].labels[0].text,
            "<ili9341 id=\"tft\"> needs an output pin, but `gpio4` has direction=\"input\""
        );
    }

    #[test]
    fn a_tft_without_a_backlight_still_lowers() {
        let src = TFT.replace(" backlight=\"$gpio6\"", "");
        let board = lower_src(&src).expect("backlight is optional");
        let tft = &board.devices[0];
        assert_eq!(tft.pins.len(), 2, "only dc and reset are claimed");
        assert_eq!(tft.args[2], "None");
    }

    #[test]
    fn one_spi_component_takes_one_display() {
        let src = TFT.replace(
            "# devices\n",
            "# devices\n\
             \x20 <ili9341 id=\"other\" spi_component=\"$tft_bus\" dc=\"$gpio4\" reset=\"$gpio5\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "spi component already used");
        assert_eq!(
            errors[0].labels[0].text,
            "<ili9341 id=\"other\"> and <ili9341 id=\"tft\"> both use <spi_component id=\"tft_bus\">"
        );
    }

    #[test]
    fn a_tft_control_pin_beside_an_led_is_a_pin_conflict() {
        let src = TFT.replace(
            "  <spi_component id=\"tft_bus\" spi=\"$spi2\" cs=\"9\" />\n",
            "  <spi_component id=\"tft_bus\" spi=\"$spi2\" cs=\"9\" />\n\
             \x20 <led id=\"red_led\" gpio=\"$gpio4\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(errors[0].message, "pin conflict");
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"red_led\"> and <ili9341 id=\"tft\"> both use GPIO4"
        );
    }
}

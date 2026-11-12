//! Lowering: walk `# peripherals`, `# devices` and `# services` in rank order,
//! find the registered element for each tag, and build the board.
//!
//! An unknown tag is reported from the schema ∩ registry, so the "known"
//! list in the message cannot drift from what actually exists.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::{Diagnostic, codes};

use crate::element::{Element, LowerCx};
use crate::model::{Board, Bus, Console, GpioDecl, Meta};
use crate::schema::Session;

/// Lower the hardware sections onto `meta`, in rank order.
///
/// Diagnostics are collected and returned together; a `Board` exists only when
/// there are none. A bus pin on a declared `<gpio>` stops the walk before the
/// devices, in the legacy order the diagnostics corpus pins. Devices are
/// lowered after the peripherals they bind to, and services last: a pass over
/// a failed one below it would only report the same missing reference twice.
pub fn lower(
    meta: Meta,
    peripherals: &[ParsedNode<'_>],
    devices: &[ParsedNode<'_>],
    services: &[ParsedNode<'_>],
    session: &Session,
) -> Result<Board, Vec<Diagnostic>> {
    let mut cx = LowerCx::with_schema(session.schema());
    cx.set_meta(&meta);
    hold_runtime_resources(&mut cx, &meta);
    let peripherals: Vec<&ParsedNode<'_>> = peripherals.iter().collect();
    lower_section("peripherals", &peripherals, session, &mut cx);
    check_bus_gpio_declarations(&mut cx);
    // Legacy stopped at the first bus/gpio conflict before a device could
    // claim the same pin, and the corpus pins that single error. Keep that
    // order: a device is not lowered onto a pin the board already called out.
    // Peripherals are all lowered first, so a `<gpio>` written after the bus
    // that uses its pin still reaches this pass.
    if cx.has_errors() {
        return cx.into_board(meta);
    }
    let devices: Vec<&ParsedNode<'_>> = devices.iter().collect();
    lower_section("devices", &devices, session, &mut cx);
    check_spi_cs_gpio_declarations(&mut cx);
    if cx.has_errors() {
        return cx.into_board(meta);
    }
    check_bus_modes(&mut cx);
    if cx.has_errors() {
        return cx.into_board(meta);
    }
    let services: Vec<&ParsedNode<'_>> = services.iter().collect();
    lower_section("services", &services, session, &mut cx);
    cx.into_board(meta)
}

/// Record what the generated `main()` takes for itself, before any element is
/// lowered.
///
/// The console binds `UART0` and, under embassy, `esp_rtos::start` takes a
/// timer group timer and an interrupt line of its own. Holding those names in
/// the graph turns a spec that asks for the same silicon into a located error
/// instead of a collision nobody sees until hardware.
fn hold_runtime_resources(cx: &mut LowerCx<'_>, meta: &Meta) {
    if meta.console == Console::Uart {
        cx.hold(
            "UART0",
            "the log console",
            "use another UART, or move the console with `<console type=\"jtag-serial\" />`",
        );
    }
    if meta.runtime == crate::model::Runtime::Embassy {
        cx.hold(
            "TIMG0.timer0",
            "the embassy tick (`esp_rtos::start`)",
            "the scheduler owns TIMG0's first timer on every embassy project",
        );
        cx.hold(
            "FROM_CPU_INTR0",
            "the embassy tick (`esp_rtos::start`)",
            "the scheduler owns this interrupt line on every embassy project",
        );
    }
}

/// A direct SPI chip select that collides with a declared `<gpio>` cannot be
/// both a GPIO declaration and the bus's raw CS line.
fn check_spi_cs_gpio_declarations(cx: &mut LowerCx<'_>) {
    let gpios: Vec<GpioDecl> = cx.gpios().to_vec();
    let mut found: Option<(std::ops::Range<usize>, String)> = None;
    for bus in cx.buses() {
        let Bus::Spi {
            id, cs: Some(cs), ..
        } = bus
        else {
            continue;
        };
        for gpio in &gpios {
            if gpio.pin == cs.pin().0 {
                found = Some((
                    gpio.span.clone(),
                    format!(
                        "<gpio id=\"{}\"> and <spi id=\"{}\"> both use GPIO{}",
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

/// A bus pin that a `<gpio>` declared is a wiring error: the gpio is a second
/// user of the pin, and the diagnostic points at the gpio (as the legacy
/// `check_bus_gpio_declarations` did).
fn check_bus_gpio_declarations(cx: &mut LowerCx<'_>) {
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

/// A blocking-only and an async-only driver on one controller cannot both be
/// right: the controller is in one mode. Report the second device instead of
/// silently picking a mode that breaks the other part at run time.
fn check_bus_modes(cx: &mut LowerCx<'_>) {
    // Collect first: reporting needs `&mut cx`, which the device list borrow
    // forbids while it is alive.
    let mut conflicts: Vec<(usize, std::ops::Range<usize>, String)> = Vec::new();
    let mut reported: Vec<usize> = Vec::new();
    for device in cx.devices().iter().filter_map(crate::model::Device::as_bus) {
        let bus = device.bus();
        if reported.contains(&bus.0) {
            continue;
        }
        let blocking = cx
            .devices()
            .iter()
            .filter_map(crate::model::Device::as_bus)
            .find(|other| other.bus() == bus && other.driver.runtimes.blocking_only());
        let asynchronous = cx
            .devices()
            .iter()
            .filter_map(crate::model::Device::as_bus)
            .find(|other| other.bus() == bus && other.driver.runtimes.async_only());
        let (Some(blocking), Some(asynchronous)) = (blocking, asynchronous) else {
            continue;
        };
        let bus_kind = match cx.bus(bus) {
            Some(Bus::I2c { .. }) => "i2c",
            Some(Bus::Spi { .. }) => "spi",
            Some(Bus::Uart { .. }) => "uart",
            None => "bus",
        };
        let bus_id = cx
            .bus(bus)
            .map(|bus| bus.id().raw.clone())
            .unwrap_or_default();
        conflicts.push((
            bus.0,
            asynchronous.id.span.clone(),
            format!(
                "<{} id=\"{}\"> needs an async bus, but <{} id=\"{}\"> needs a blocking one; \
                 move one device to another <{bus_kind}> than <{bus_kind} id=\"{bus_id}\">",
                asynchronous.tag, asynchronous.id.raw, blocking.tag, blocking.id.raw
            ),
        ));
        reported.push(bus.0);
    }
    for (_, span, label) in conflicts {
        cx.report(
            codes::DEVICE_BUS_MODE_CONFLICT,
            span,
            "bus mode conflict",
            label,
        );
    }
}

fn lower_section(
    section: &str,
    nodes: &[&ParsedNode<'_>],
    session: &Session,
    cx: &mut LowerCx<'_>,
) {
    for node in nodes {
        match element_for(section, node.tag_name, session) {
            Some(element) => {
                // One element is one transaction: a bus whose second pin
                // conflicts must not leave its first pin (or its peripheral)
                // claimed, or the next element reports a conflict on a pin
                // nobody owns. The checkpoint is taken here, immediately
                // before the single `lower` call, and rolled back only when it
                // reports.
                let checkpoint = cx.checkpoint();
                if element.lower(node, cx).is_err() {
                    cx.rollback(checkpoint);
                }
            }
            None => {
                let known = known_tags(section, session);
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
                    _ => codes::DEVICE_UNSUPPORTED,
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
fn element_for<'a>(section: &str, tag: &str, session: &'a Session) -> Option<&'a dyn Element> {
    let definition = session.schema().element(tag)?;
    if definition.section != section {
        return None;
    }
    session.element(tag)
}

/// The tags that can appear in `section`: schema elements with an
/// implementation, in schema order.
fn known_tags(section: &str, session: &Session) -> Vec<String> {
    session
        .schema()
        .elements_in(section)
        .filter(|element| session.element(&element.tag).is_some())
        .map(|element| element.tag.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Lowered;
    use crate::model::{
        BusDevice, BusTransport, Console, Device, Direction, Driver, I2cTransport, OwnerKind, Pin,
        PinDeviceKind, PinUse, ProtocolService, Runtime, Service,
    };
    use espf::parser::parse_espf;
    use espforge_diagnostics::Severity;

    const BLINK: &str = "# espforge\n\
        \x20 <project name=\"blink\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n\
        # devices\n\
        \x20 <led id=\"red_led\" gpio=\"$gpio18\" />\n\
        # services\n";

    const BUTTON: &str = "# espforge\n\
        \x20 <project name=\"button\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <gpio id=\"gpio7\" pin=\"7\" direction=\"output\" />\n\
        \x20 <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n\
        # devices\n\
        \x20 <led id=\"red_led\" gpio=\"$gpio7\" active_low=\"false\" />\n\
        \x20 <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n\
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
        let section = |name: &str| parsed.sections.get(name).map(Vec::as_slice).unwrap_or(&[]);
        lower(
            meta,
            section("peripherals"),
            section("devices"),
            section("services"),
            &Session::bundled(),
        )
    }

    /// The same, against a named chip: the caller's `Meta` chooses the pin
    /// table, not the `<chip>` text inside the spec.
    fn lower_on(chip: &str, src: &str) -> Result<Board, Vec<Diagnostic>> {
        let mut meta = meta();
        meta.chip = chip.into();
        lower_src_with(src, meta)
    }

    #[test]
    fn blink_lowers_to_one_led_on_pin_18() {
        let board = lower_src(BLINK).expect("blink lowers");
        assert_eq!(board.devices.len(), 1);
        let Device::Pin(led) = &board.devices[0] else {
            panic!("blink lowers to a led");
        };
        assert_eq!(led.tag, "led");
        assert_eq!(led.id.raw, "red_led");
        assert_eq!(led.id.rust_name(), "red_led");
        assert_eq!(led.pin.pin(), Pin(18));
        assert_eq!(led.kind, PinDeviceKind::Output { active_low: false });
        assert_eq!(led.overlay_attr("pin").as_deref(), Some("18"));
        assert_eq!(led.overlay_attr("color"), None);
        assert_eq!(
            board.resources.claim_of(Pin(18)).unwrap().owner.kind,
            OwnerKind::Pin("led".into())
        );
    }

    #[test]
    fn a_gpio_without_a_direction_can_still_drive_an_led() {
        let src = BLINK.replace(" direction=\"output\"", "");
        let board = lower_src(&src).expect("no direction is allowed");
        assert_eq!(board.devices.len(), 1);
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
            "# devices\n",
            "# devices\n  <gpio id=\"extra\" pin=\"7\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNSUPPORTED);
        assert!(
            errors[0]
                .message
                .contains("`<gpio>` is not a known element in `# devices`")
        );
    }

    /// The unknown-tag list for `# devices` walks `registry::element`, so the
    /// recipe door must show up in it: a device the schema does not know is
    /// told which devices exist.
    #[test]
    fn an_unknown_device_names_the_recipe_elements() {
        let src = BLINK.replace("# devices\n", "# devices\n  <pwm id=\"pwm0\" />\n");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<pwm>` is not a known element in `# devices`; known: `<led>`, `<button>`, `<ssd1306>`, `<ili9341>`, `<scd4x>`, `<adxl345>`"
        );
    }

    #[test]
    fn button_lowers_beside_an_led_and_claims_its_input_gpio() {
        let board = lower_src(BUTTON).expect("button lowers");
        assert_eq!(board.devices.len(), 2);

        let Device::Pin(led) = &board.devices[0] else {
            panic!("the first device is the led");
        };
        assert_eq!(led.tag, "led");
        assert_eq!(led.id.raw, "red_led");
        assert_eq!(led.pin.pin(), Pin(7));

        let Device::Pin(button) = &board.devices[1] else {
            panic!("the second device is the button");
        };
        assert_eq!(button.tag, "button");
        assert_eq!(button.id.raw, "button");
        assert_eq!(button.pin.pin(), Pin(9));
        assert_eq!(button.kind, PinDeviceKind::Input { pull_up: true });
        assert_eq!(button.overlay_attr("pin").as_deref(), Some("9"));
        assert_eq!(button.overlay_attr("color"), None);

        let led_claim = board.resources.claim_of(Pin(7)).unwrap();
        assert_eq!(led_claim.owner.kind, OwnerKind::Pin("led".into()));
        assert_eq!(led_claim.need, PinUse::Output);
        let button_claim = board.resources.claim_of(Pin(9)).unwrap();
        assert_eq!(button_claim.owner.kind, OwnerKind::Pin("button".into()));
        assert_eq!(button_claim.need, PinUse::Input);
    }

    /// `pin="18"` and `gpio="$gpio18"` are one pin: the same claim, the same
    /// runtime type and the same emitted field.
    #[test]
    fn a_led_pin_number_and_a_gpio_reference_lower_to_the_same_pin() {
        let numeric = lower_src(&BLINK.replace(
            "<led id=\"red_led\" gpio=\"$gpio18\" />",
            "<led id=\"red_led\" pin=\"18\" />",
        ))
        .expect("pin= lowers");
        let by_reference = lower_src(BLINK).expect("a gpio reference lowers");

        let Device::Pin(numeric) = &numeric.devices[0] else {
            panic!("the numeric pin lowers to a pin device");
        };
        let Device::Pin(by_reference) = &by_reference.devices[0] else {
            panic!("the referenced pin lowers to a pin device");
        };
        assert_eq!(numeric.pin.pin(), Pin(18));
        assert_eq!(numeric.pin.pin(), by_reference.pin.pin());
        assert_eq!(numeric.runtime, by_reference.runtime);
        assert_eq!(numeric.kind, by_reference.kind);
    }

    #[test]
    fn a_button_pin_may_leave_its_direction_undeclared() {
        let src = BUTTON.replace(
            "<gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />",
            "<gpio id=\"gpio9\" pin=\"9\" />",
        );
        let board = lower_src(&src).expect("no direction is allowed");
        assert_eq!(board.devices.len(), 2);
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
        let Device::Pin(button) = &board.devices[1] else {
            panic!("the second device is the button");
        };
        assert_eq!(button.tag, "button");
        assert_eq!(button.kind, PinDeviceKind::Input { pull_up: false });

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
        # devices\n\
        # services\n";

    #[test]
    fn i2c_lowers_to_a_bus() {
        let board = lower_src(I2C).expect("i2c lowers");
        assert!(board.devices.is_empty());
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
    fn i2c_sda_and_scl_on_one_pin_are_i2c_wiring() {
        let errors = lower_src(&I2C.replace("scl=\"7\"", "scl=\"6\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_I2C_WIRING);
        assert_eq!(errors[0].message, "i2c wiring");
    }

    #[test]
    fn a_bus_pin_on_a_declared_gpio_points_at_the_gpio() {
        let src = I2C.replace(
            "  <i2c id=\"i2c0\"",
            "  <gpio id=\"g\" pin=\"6\" />\n  <i2c id=\"i2c0\"",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(
            errors[0].labels[0].text,
            "<gpio id=\"g\"> and <i2c id=\"i2c0\"> both use GPIO6"
        );
    }

    const SPI: &str = "# espforge\n\
        \x20 <project name=\"spi\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" mode=\"0\" frequency=\"100kHz\" cs=\"9\" />\n\
        # devices\n\
        # services\n";

    #[test]
    fn spi_lowers_to_a_full_duplex_bus_and_a_cs() {
        let board = lower_src(SPI).expect("spi lowers");
        let Bus::Spi {
            peripheral,
            sclk,
            mosi,
            miso,
            khz,
            mode,
            cs,
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
        assert_eq!(cs.as_ref().unwrap().pin().0, 9);
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
            "<gpio id=\"g\"> and <spi id=\"spi2\"> both use GPIO9"
        );
    }

    const UART: &str = "# espforge\n\
        \x20 <project name=\"uart\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" baud_rate=\"9600\" />\n\
        # devices\n\
        # services\n";

    #[test]
    fn uart_lowers_to_a_port_and_its_framing() {
        let board = lower_src(UART).expect("uart lowers");
        let Bus::Uart {
            peripheral,
            tx,
            rx,
            baud,
            framing,
            ..
        } = &board.resources.buses()[0]
        else {
            panic!("the bus is a uart bus");
        };
        assert_eq!(peripheral, "UART1");
        assert_eq!((tx.pin().0, rx.pin().0), (6, 5));
        assert_eq!(*baud, 9600);
        assert_eq!(framing.parity, crate::model::Parity::None);
        assert_eq!(framing.stop_bits, crate::model::StopBits::One);
        assert_eq!(
            board.resources.claim_of(Pin(6)).unwrap().need,
            PinUse::Unspecified
        );
    }

    #[test]
    fn uart_tx_and_rx_on_one_pin_are_uart_wiring() {
        let errors = lower_src(&UART.replace("rx=\"5\"", "rx=\"6\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_UART_WIRING);
        assert_eq!(errors[0].message, "uart wiring");
    }

    #[test]
    fn two_uarts_on_one_peripheral_are_a_peripheral_clash() {
        let src = UART.replace(
            "  <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" baud_rate=\"9600\" />\n",
            "  <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" baud_rate=\"9600\" />\n  <uart id=\"other\" uart=\"UART1\" tx=\"7\" rx=\"8\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PERIPHERAL);
    }

    #[test]
    fn a_uart_on_the_console_port_is_refused() {
        let src = UART.replace("uart=\"UART1\"", "uart=\"UART0\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_RUNTIME_HELD);
        assert_eq!(errors[0].message, "UART0 is held by the log console");
        assert!(
            errors[0].labels[0].text.contains("jtag-serial"),
            "{}",
            errors[0].labels[0].text
        );
    }

    #[test]
    fn moving_the_console_off_uart_frees_uart0() {
        let src = UART.replace("uart=\"UART1\"", "uart=\"UART0\"");
        let mut meta = meta();
        meta.console = Console::JtagSerial;
        let board = lower_src_with(&src, meta).expect("uart0 is free off the console");
        assert_eq!(board.resources.buses().len(), 1);
    }

    #[test]
    fn the_runtime_holds_what_the_generated_main_binds() {
        let blocking = lower_src(BLINK).expect("blink lowers");
        assert_eq!(
            blocking
                .resources
                .held_resources()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["UART0"]
        );

        let src = BLINK.replace(
            "<runtime type=\"blocking\" />",
            "<runtime type=\"embassy\" />",
        );
        let mut meta = meta();
        meta.runtime = Runtime::Embassy;
        let embassy = lower_src_with(&src, meta).expect("an embassy blink lowers");
        assert_eq!(
            embassy
                .resources
                .held_resources()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["FROM_CPU_INTR0", "TIMG0.timer0", "UART0"]
        );
    }

    #[test]
    fn a_zero_baud_rate_is_refused() {
        let errors = lower_src(&UART.replace("baud_rate=\"9600\"", "baud_rate=\"0\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::UART_INVALID_BAUD);
    }

    #[test]
    fn the_gpio_direction_is_carried_as_a_domain_value() {
        let src = BLINK.replace("direction=\"output\"", "direction=\"sideways\"");
        let parsed = parse_espf(&src);
        let mut cx = LowerCx::new();
        let peripherals: Vec<_> = parsed.sections["peripherals"].iter().collect();
        lower_section("peripherals", &peripherals, &Session::bundled(), &mut cx);
        assert!(cx.gpio("gpio18").is_none(), "nothing is recorded on error");

        let parsed = parse_espf(BLINK);
        let mut cx = LowerCx::new();
        let peripherals: Vec<_> = parsed.sections["peripherals"].iter().collect();
        lower_section("peripherals", &peripherals, &Session::bundled(), &mut cx);
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
        # devices\n\
        # services\n\
        \x20 <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n";

    fn embassy_meta() -> Meta {
        Meta {
            runtime: Runtime::Embassy,
            ..meta()
        }
    }

    /// Put one client after the station in [`WIFI_STATION`]'s `# services`:
    /// a client names the provider, so the provider must lower first.
    fn with_service(service: &str) -> String {
        WIFI_STATION.replace(
            "  <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n",
            &format!(
                "  <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n  {service}\n"
            ),
        )
    }

    #[test]
    fn a_station_lowers_under_its_radio() {
        let board = lower_src_with(WIFI_STATION, embassy_meta()).expect("the station lowers");
        let Service::WifiStation(station) = &board.services[0] else {
            panic!("the service is a wifi station");
        };
        assert_eq!(station.id.raw, "home");
        assert_eq!(station.radio.raw, "radio");
        assert_eq!(station.ssid, "Wokwi-GUEST");
        assert_eq!(
            station.password,
            crate::services::wifi_station::Password::Open
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
        let Service::WifiStation(station) = &board.services[0] else {
            panic!("the service is a wifi station");
        };
        assert_eq!(
            station.password,
            crate::services::wifi_station::Password::Env("WIFI_PASSWORD".into())
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
    fn field_of(service: &ProtocolService, name: &str) -> String {
        service
            .config_fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, expr)| expr.text())
            .unwrap_or_else(|| panic!("`{name}` is not a config field of <{}>", service.tag))
    }

    /// The const value is baked into the types, not written into the config.
    #[test]
    fn http_client_connections_parameterize_the_client_types() {
        let src = with_service("<http_client id=\"client\" network=\"$home\" connections=\"2\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("the client lowers");
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
        let ty = client.ty.text().replace(' ', "");
        let new = client.new.text().replace(' ', "");
        let storage = client.storage.as_ref().unwrap().text().replace(' ', "");
        assert_eq!(ty, "espforge_runtime::HttpClient::<2>");
        assert_eq!(new, "espforge_runtime::HttpClient::<2>::new_in");
        assert_eq!(storage, "espforge_runtime::HttpClientStorage::<2>");
        assert!(
            !client
                .config_fields
                .iter()
                .any(|(field, _)| field == "connections"),
            "the slot count is not a config member"
        );
        assert_eq!(field_of(client, "rx_buffer_size"), "2048u32 as usize");
    }

    /// The default slot count keeps the bare types, so an unchanged project
    /// generates exactly what it generated before.
    #[test]
    fn one_connection_generates_the_bare_types() {
        for spec in [
            "<http_client id=\"client\" network=\"$home\" />",
            "<http_client id=\"client\" network=\"$home\" connections=\"1\" />",
        ] {
            let src = with_service(spec);
            let board = lower_src_with(&src, embassy_meta()).expect("the client lowers");
            let Service::Protocol(client) = &board.services[1] else {
                panic!("the service is a protocol client");
            };
            assert_eq!(client.ty.text(), "espforge_runtime::HttpClient", "{spec}");
            assert_eq!(
                client.new.text(),
                "espforge_runtime::HttpClient::new_in",
                "{spec}"
            );
            assert_eq!(
                client.storage.as_ref().map(|ty| ty.text()).as_deref(),
                Some("espforge_runtime::HttpClientStorage"),
                "{spec}"
            );
        }
    }

    #[test]
    fn a_connection_count_outside_the_window_is_refused() {
        for count in ["0", "5"] {
            let src = with_service(&format!(
                "<http_client id=\"client\" network=\"$home\" connections=\"{count}\" />"
            ));
            let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
            assert_eq!(errors[0].code, codes::SERVICE_INVALID, "{count}");
            assert!(
                errors[0].labels[0].text.contains("1..=4"),
                "{count}: {}",
                errors[0].labels[0].text
            );
        }
    }

    #[test]
    fn an_http_client_lowers_over_the_stations_stack() {
        let src = with_service("<http_client id=\"client\" network=\"$home\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("the client lowers");
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
        assert_eq!(client.id.raw, "client");
        assert_eq!(client.network.raw, "home");
        assert_eq!(client.tag, "http_client");
        assert_eq!(client.feature, "http-client");
        assert_eq!(client.ty.text(), "espforge_runtime::HttpClient");
        assert_eq!(client.new.text(), "espforge_runtime::HttpClient::new_in");
        assert_eq!(
            client.storage.as_ref().map(|ty| ty.text()).as_deref(),
            Some("espforge_runtime::HttpClientStorage")
        );
        assert_eq!(
            client.config.as_ref().map(|ty| ty.text()).as_deref(),
            Some("espforge_runtime::HttpClientConfig")
        );
        assert_eq!(field_of(client, "rx_buffer_size"), "2048u32 as usize");
        assert_eq!(field_of(client, "tx_buffer_size"), "1024u32 as usize");
        assert_eq!(field_of(client, "timeout_ms"), "10000u32 as u64");
        assert!(client.extra_args.is_empty());
        assert!(board.warnings.is_empty(), "no warning without insecure");
    }

    #[test]
    fn a_service_network_must_be_a_service_that_has_a_stack() {
        let src = with_service("<http_client id=\"client\" network=\"$nope\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNKNOWN_NETWORK);
        assert_eq!(errors[0].message, "unknown network interface");
        assert_eq!(
            errors[0].labels[0].text,
            "<http_client id=\"client\"> refers to `nope`, which provides no network interface in `# services`"
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
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
        assert_eq!(client.extra_args[0].text(), "true");
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
            "`<ntp_client>` is not a known element in `# services`; known: `<wifi_station>`, `<http_client>`, `<https_client>`, `<ws_client>`, `<wss_client>`, `<ntp>`"
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
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
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
    fn a_ws_client_network_must_be_a_service_that_has_a_stack() {
        let src = with_service("<ws_client id=\"ws\" network=\"$nope\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNKNOWN_NETWORK);
        assert_eq!(
            errors[0].labels[0].text,
            "<ws_client id=\"ws\"> refers to `nope`, which provides no network interface in `# services`"
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
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
        assert_eq!(client.extra_args[0].text(), "true");
        assert_eq!(client.heap_bytes, Some(98304));
        assert_eq!(field_of(client, "rx_buffer_size"), "4096u32 as usize");
        assert_eq!(board.max_service_heap(), Some(98304));
    }

    #[test]
    fn an_ntp_client_lowers_over_the_stations_stack() {
        let src = with_service("<ntp id=\"clock\" network=\"$home\" />");
        let board = lower_src_with(&src, embassy_meta()).expect("the ntp client lowers");
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
        assert_eq!(client.id.raw, "clock");
        assert_eq!(client.network.raw, "home");
        assert_eq!(client.tag, "ntp");
        assert_eq!(client.feature, "ntp");
        assert_eq!(client.ty.text(), "espforge_runtime::NtpClient");
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
        let Service::Protocol(client) = &board.services[1] else {
            panic!("the service is a protocol client");
        };
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
    fn an_ntp_client_network_must_be_a_service_that_has_a_stack() {
        let src = with_service("<ntp id=\"clock\" network=\"$nope\" />");
        let errors = lower_src_with(&src, embassy_meta()).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_UNKNOWN_NETWORK);
        assert_eq!(
            errors[0].labels[0].text,
            "<ntp id=\"clock\"> refers to `nope`, which provides no network interface in `# services`"
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
        # devices\n\
        \x20 <ssd1306 id=\"oled\" bus=\"$i2c0\" address=\"0x3C\" size=\"128x32\" rotation=\"180\" />\n\
        # services\n";

    #[test]
    fn an_ssd1306_lowers_onto_its_i2c_bus() {
        let board = lower_src(OLED).expect("the oled lowers");
        assert_eq!(board.devices.len(), 1);
        let oled = &board.devices[0];
        let oled = oled.as_bus().expect("the display is a bus device");
        assert_eq!(oled.tag, "ssd1306");
        assert_eq!(oled.id.raw, "oled");
        assert_eq!(Some(oled.bus()), board.resources.bus_by_name("i2c0"));
        let BusTransport::I2c(i2c) = &oled.transport else {
            panic!("the display is on an I2C bus");
        };
        assert_eq!(i2c.address, 0x3C);
        // The recipe's slots are filled from the values as written.
        assert_eq!(
            oled.driver.ty,
            crate::model::RustType::parse(
                "espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x32>"
            )
            .unwrap()
        );
        assert_eq!(
            oled.driver.args,
            [
                crate::model::RustExpr::parse("espforge_drivers::Oled128x32").unwrap(),
                crate::model::RustExpr::parse("espforge_drivers::DisplayRotation::Rotate180")
                    .unwrap()
            ]
        );
        assert_eq!(oled.driver.feature, "ssd1306");
        assert_eq!(board.driver_features(), ["ssd1306"]);
        assert_eq!(board.driver_deps(), ["embedded-graphics"]);
    }

    /// A composite and its companion: one spec element, two devices.
    const COMPOSED_DRIVERS: &str = r##"
driver "panel" {
    format_version 2
    summary "Panel with a touch companion"
    category "display"
    dependency "embedded-graphics" { version "0.8.1" }
    bus "spi" { }
    pin "dc" direction="output"
    pin "touch_cs" direction="output"
    pin "touch_irq" direction="input" pull="up"
    compose "touch" {
        attr "cs" from="touch_cs"
        attr "irq" from="touch_irq"
        attr "init" value="deferred"
    }
    codegen {
        feature "panel"
        runtime "blocking" {
            type "panel::Panel"
            constructor "panel::Panel::new({bus_device}, {cs_pin}, {dc})"
        }
    }
    example "minimal" #"<panel id="tft" bus="$spi2" cs="9" dc="$gpio4" />"#
}

driver "touch" {
    format_version 2
    summary "Touch controller"
    category "sensor"
    dependency "embedded-graphics" { version "0.8.1" }
    bus "spi" { }
    pin "irq" direction="input" pull="up"
    codegen {
        feature "touch"
        runtime "blocking" {
            type "touch::Touch"
            constructor "touch::Touch::new({bus_device}, {cs_pin}, {irq}, {init})"
        }
        map "init" {
            "boot" { expr "touch::Init::Boot" }
            "deferred" { expr "touch::Init::Deferred" }
        }
    }
    attr "init" kind="choice" default="boot" { choices "boot" "deferred" }
    example "minimal" #"<touch id="t" bus="$spi2" cs="10" irq="$gpio5" />"#
}
"##;

    const COMPOSED: &str = "# espforge\n\
        \x20 <project name=\"panel\" />\n\
        \x20 <chip type=\"esp32c3\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" />\n\
        \x20 <gpio id=\"gpio4\" pin=\"4\" direction=\"output\" />\n\
        \x20 <gpio id=\"gpio5\" pin=\"5\" direction=\"output\" />\n\
        \x20 <gpio id=\"gpio10\" pin=\"10\" direction=\"input\" />\n\
        # devices\n\
        \x20 <panel id=\"tft\" bus=\"$spi2\" cs=\"9\" dc=\"$gpio4\" touch_cs=\"$gpio5\" touch_irq=\"$gpio10\" />\n\
        # services\n";

    fn lower_composed() -> Result<Board, Vec<Diagnostic>> {
        let session = Session::compose(&[(
            "test/composed.kdl".to_string(),
            COMPOSED_DRIVERS.to_string(),
        )])
        .expect("the fixture drivers load");
        let parsed = parse_espf(COMPOSED);
        let section = |name: &str| parsed.sections.get(name).map(Vec::as_slice).unwrap_or(&[]);
        lower(
            meta(),
            section("peripherals"),
            section("devices"),
            section("services"),
            &session,
        )
    }

    #[test]
    fn a_composed_part_lowers_to_its_own_device() {
        let board = lower_composed().expect("the composite lowers");
        let ids: Vec<&str> = board
            .devices
            .iter()
            .map(|device| device.id().raw.as_str())
            .collect();
        assert_eq!(ids, ["tft", "tft_touch"]);

        let touch = board.devices[1].as_bus().expect("the part is a bus device");
        assert_eq!(touch.tag, "touch");
        assert_eq!(Some(touch.bus()), board.resources.bus_by_name("spi2"));
        // `attr "init" value="deferred"` reaches the part's mapped slot.
        assert_eq!(touch.driver.init, crate::model::Init::Deferred);
    }

    #[test]
    fn a_part_pin_comes_from_the_composite() {
        let board = lower_composed().expect("the composite lowers");
        let BusTransport::Spi(panel) = &board.devices[0].as_bus().unwrap().transport else {
            panic!("the panel is on SPI");
        };
        assert_eq!(panel.cs.pin(), Pin(9));

        let BusTransport::Spi(touch) = &board.devices[1].as_bus().unwrap().transport else {
            panic!("the touch controller is on SPI");
        };
        assert_eq!(touch.cs.pin(), Pin(5), "`cs` is the composite's `touch_cs`");
        assert_eq!(
            board.resources.claim_of(Pin(5)).unwrap().owner.kind,
            OwnerKind::Device("touch".into())
        );
        assert_eq!(
            board.resources.claim_of(Pin(10)).unwrap().owner.kind,
            OwnerKind::Device("touch".into())
        );
    }

    /// A display bound to the wrong bus kind is refused at the `bus`
    /// attribute: the recipe declares `bus "i2c"`, so an SPI peripheral
    /// cannot reach emission and fail in `rustc`.
    #[test]
    fn an_ssd1306_on_an_spi_bus_is_refused() {
        let src = OLED
            .replace(
                "  <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n",
                "  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" />\n",
            )
            .replace("bus=\"$i2c0\"", "bus=\"$spi2\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "display needs a i2c bus");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> binds to `spi2`, which is an <spi>, but this display drives an <i2c>"
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
    fn a_scd4x_lowers_onto_its_i2c_bus() {
        let src = OLED.replace(
            "<ssd1306 id=\"oled\" bus=\"$i2c0\" address=\"0x3C\" size=\"128x32\" rotation=\"180\" />",
            "<scd4x id=\"air\" bus=\"$i2c0\" address=\"0x62\" />",
        );
        let board = lower_src(&src).expect("the sensor lowers");
        let Device::Bus(air) = &board.devices[0] else {
            panic!("the sensor is a bus device");
        };
        assert_eq!(air.tag, "scd4x");
        assert_eq!(air.id.raw, "air");
        assert_eq!(
            air.driver.ty,
            crate::model::RustType::parse("espforge_drivers::Scd4x").unwrap()
        );
        assert_eq!(
            air.driver.new,
            crate::model::RustPath::parse("espforge_drivers::Scd4x::new").unwrap()
        );
        assert!(air.driver.args.is_empty());
        assert!(air.driver.pins.is_empty());
        assert_eq!(board.driver_features(), ["scd4x"]);
        assert!(board.driver_deps().is_empty());
    }

    /// A display and a sensor on one controller: both bind to the same
    /// peripheral and carry their own address.
    #[test]
    fn a_display_and_a_sensor_share_one_bus() {
        let src = OLED.replace(
            "# services",
            "  <scd4x id=\"air\" bus=\"$i2c0\" address=\"0x62\" />\n# services",
        );
        let board = lower_src(&src).expect("one bus, two devices");
        assert_eq!(board.devices.len(), 2);
        assert_eq!(board.driver_features(), ["ssd1306", "scd4x"]);
        let bus = board.resources.bus_by_name("i2c0");
        assert_eq!(Some(board.devices[0].as_bus().unwrap().bus()), bus);
        assert_eq!(Some(board.devices[1].as_bus().unwrap().bus()), bus);
        let BusTransport::I2c(air) = &board.devices[1].as_bus().unwrap().transport else {
            panic!("the sensor is on an I2C bus");
        };
        assert_eq!(air.address, 0x62);
    }

    /// An addressed driver cannot reach emission without an address: the
    /// schema's own `required` rule refuses it first.
    #[test]
    fn a_display_needs_its_address() {
        let src = OLED.replace(" address=\"0x3C\"", "");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "invalid ssd1306");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> is missing `address`"
        );
    }

    /// The sensor's address is required too, and the schema words the miss the
    /// same way for both I2C drivers.
    #[test]
    fn a_sensor_needs_its_address() {
        let src = OLED
            .replace(
                "<ssd1306 id=\"oled\" bus=\"$i2c0\" address=\"0x3C\" size=\"128x32\" rotation=\"180\" />",
                "<scd4x id=\"air\" bus=\"$i2c0\" />",
            );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "invalid scd4x");
        assert_eq!(
            errors[0].labels[0].text,
            "<scd4x id=\"air\"> is missing `address`"
        );
    }

    /// A fixed-address part declares another address: that is a wiring error,
    /// and only lowering can see it.
    #[test]
    fn a_sensor_on_another_address_is_refused() {
        let src = OLED.replace(
            "<ssd1306 id=\"oled\" bus=\"$i2c0\" address=\"0x3C\" size=\"128x32\" rotation=\"180\" />",
            "<scd4x id=\"air\" bus=\"$i2c0\" address=\"0x48\" />",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "sensor address mismatch");
        assert_eq!(
            errors[0].labels[0].text,
            "<scd4x id=\"air\"> is fixed at address 0x62, but the spec declares 0x48"
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
    fn a_device_bus_must_name_a_peripheral_of_its_kind() {
        let errors = lower_src(&OLED.replace("bus=\"$i2c0\"", "bus=\"$nope\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNKNOWN_COMPONENT);
        assert_eq!(errors[0].message, "unknown bus");
        assert_eq!(
            errors[0].labels[0].text,
            "<ssd1306 id=\"oled\"> refers to `nope`, which is not an <i2c> in `# peripherals`"
        );
    }

    #[test]
    fn an_ssd1306_id_must_become_a_rust_identifier() {
        let errors = lower_src(&OLED.replace("id=\"oled\"", "id=\"9led\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_INVALID_ID);
        assert_eq!(errors[0].message, "invalid device id");
        assert!(errors[0].labels[0].text.contains("valid Rust identifier"));
    }

    /// A dual-mode driver follows the project's scheduler: the same spec is
    /// blocking on a blocking board and async on an embassy one.
    #[test]
    fn a_dual_mode_display_follows_the_scheduler() {
        let blocking = lower_src(OLED).expect("the display lowers");
        let bus = blocking.devices[0]
            .as_bus()
            .expect("the display is a bus device")
            .bus();
        assert_eq!(blocking.bus_mode(bus), crate::model::BusMode::Blocking);

        let embassy = lower_src_with(OLED, embassy_meta()).expect("an embassy board lowers");
        let bus = embassy.devices[0]
            .as_bus()
            .expect("the display is a bus device")
            .bus();
        assert_eq!(embassy.bus_mode(bus), crate::model::BusMode::Async);
    }

    /// One controller cannot be in both modes, so a blocking-only and an
    /// async-only driver on one bus is a diagnostic, not a silent guess.
    #[test]
    fn a_blocking_and_an_async_device_on_one_bus_are_refused() {
        use crate::model::{Bus, Ident, Owner, OwnerKind, Pin, PinUse, RuntimeSet};

        let mut cx = LowerCx::new();
        // Pin claims run the chip's pin table, so the context needs a chip.
        cx.set_meta(&meta());
        let owner = Owner::new(OwnerKind::I2c, Ident::new("i2c0", 0..0));
        let sda = cx
            .claim_pin(Pin(6), owner.clone(), 0..0, PinUse::Bidirectional)
            .expect("sda is free");
        let scl = cx
            .claim_pin(Pin(7), owner, 0..0, PinUse::Bidirectional)
            .expect("scl is free");
        let bus = cx.add_bus(Bus::I2c {
            id: Ident::new("i2c0", 0..0),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
        });
        for (id, runtimes) in [
            ("oled", RuntimeSet::BLOCKING),
            (
                "future",
                RuntimeSet {
                    blocking: false,
                    asynchronous: true,
                },
            ),
        ] {
            cx.push_device(Device::Bus(BusDevice {
                id: Ident::new(id, 0..0),
                tag: "ssd1306".into(),
                driver: Driver {
                    feature: "ssd1306".into(),
                    deps: Vec::new(),
                    ty: crate::model::RustType::parse(
                        "espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>",
                    )
                    .unwrap(),
                    new: crate::model::RustPath::parse("espforge_drivers::Ssd1306Display::new")
                        .unwrap(),
                    pins: Vec::new(),
                    args: Vec::new(),
                    factory_args: Vec::new(),
                    runtimes,
                    init: crate::model::Init::Boot,
                    async_ty: None,
                    async_new: None,
                    async_feature: None,
                    package: None,
                    expose: Vec::new(),
                    dependencies: Vec::new(),
                    manifest: None,
                },
                transport: BusTransport::I2c(I2cTransport {
                    bus,
                    address: 0x3C,
                    endpoint: None,
                    async_endpoint: None,
                }),
            }));
        }

        check_bus_modes(&mut cx);
        let errors = cx.into_board(meta()).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].code, codes::DEVICE_BUS_MODE_CONFLICT);
        assert_eq!(errors[0].message, "bus mode conflict");
        assert!(
            errors[0].labels[0].text.contains("<ssd1306 id=\"future\">"),
            "{:?}",
            errors[0].labels
        );
    }

    #[test]
    fn an_unknown_device_tag_names_the_displays() {
        let src = OLED.replace("<ssd1306 id=\"oled\"", "<st7789 id=\"lcd\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_UNSUPPORTED);
        assert_eq!(
            errors[0].message,
            "`<st7789>` is not a known element in `# devices`; known: `<led>`, `<button>`, `<ssd1306>`, `<ili9341>`, `<scd4x>`, `<adxl345>`"
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
        # devices\n\
        \x20 <ili9341 id=\"tft\" bus=\"$spi2\" cs=\"9\" dc=\"$gpio4\" reset=\"$gpio5\" backlight=\"$gpio6\" rotation=\"90\" />\n\
        # services\n";

    #[test]
    fn an_ili9341_lowers_and_claims_its_control_pins() {
        let board = lower_src(TFT).expect("the tft lowers");
        assert_eq!(board.devices.len(), 1);
        let Device::Bus(tft) = &board.devices[0] else {
            panic!("the tft is a bus device");
        };
        assert_eq!(tft.tag, "ili9341");
        assert_eq!(tft.id.raw, "tft");
        assert_eq!(Some(tft.bus()), board.resources.bus_by_name("spi2"));
        let BusTransport::Spi(spi) = &tft.transport else {
            panic!("the tft is on an SPI bus");
        };
        assert_eq!(spi.cs.pin(), Pin(9));
        let pins: Vec<(&str, u8)> = tft
            .driver
            .pins
            .iter()
            .map(|(arg, pin)| (arg.as_str(), pin.pin().0))
            .collect();
        assert_eq!(pins, [("dc", 4), ("reset", 5), ("backlight", 6)]);
        for pin in [4, 5, 6] {
            let claim = board.resources.claim_of(Pin(pin)).unwrap();
            assert_eq!(claim.owner.kind, OwnerKind::Device("ili9341".into()));
            assert_eq!(claim.need, PinUse::Output);
        }
        // The control lines are the recipe's pin templates, filled in.
        assert_eq!(
            tft.driver.args[0],
            crate::model::RustExpr::parse(
                "esp_hal::gpio::Output::new(unsafe { registry.peripherals.GPIO4.clone_unchecked() }, esp_hal::gpio::Level::Low, esp_hal::gpio::OutputConfig::default())"
            )
            .unwrap()
        );
        // An optional pin is an `Option` the driver takes.
        let optional = tft.driver.args[2].text();
        assert!(optional.starts_with("Some"), "{optional}");
        assert_eq!(
            tft.driver.args[3],
            crate::model::RustExpr::parse("espforge_drivers::Orientation::Landscape").unwrap()
        );
        assert_eq!(
            tft.driver.args[4],
            crate::model::RustExpr::parse("espforge_drivers::ColorOrder::Bgr").unwrap()
        );
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
        let tft = board.devices[0].as_bus().expect("the tft is a bus device");
        assert_eq!(tft.driver.pins.len(), 2, "only dc and reset are claimed");
        assert_eq!(tft.driver.args[2].text(), "None");
    }

    /// Two displays on one SPI controller, each with its own chip select, are
    /// the ordinary case now: there is no shared handle to consume.
    #[test]
    fn two_displays_can_share_one_spi_bus() {
        let src = TFT
            .replace(
                "  <gpio id=\"gpio6\" pin=\"6\" direction=\"output\" />\n",
                "  <gpio id=\"gpio6\" pin=\"6\" direction=\"output\" />\n\
                 \x20 <gpio id=\"gpio20\" pin=\"20\" direction=\"output\" />\n\
                 \x20 <gpio id=\"gpio21\" pin=\"21\" direction=\"output\" />\n",
            )
            .replace(
                "# services",
                "  <ili9341 id=\"other\" bus=\"$spi2\" cs=\"10\" dc=\"$gpio20\" reset=\"$gpio21\" />\n# services",
            );
        let board = lower_src(&src).expect("one bus, two chip selects");
        assert_eq!(board.devices.len(), 2);
        let pins: Vec<u8> = board
            .devices
            .iter()
            .filter_map(|device| match &device.as_bus()?.transport {
                BusTransport::Spi(spi) => Some(spi.cs.pin().0),
                BusTransport::I2c(_) | BusTransport::Uart(_) => None,
            })
            .collect();
        assert_eq!(pins, [9, 10]);
    }

    #[test]
    fn a_tft_control_pin_beside_an_led_is_a_pin_conflict() {
        let src = TFT.replace(
            "# devices\n",
            "# devices\n  <led id=\"red_led\" gpio=\"$gpio4\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(errors[0].message, "pin conflict");
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"red_led\"> and <ili9341 id=\"tft\"> both use GPIO4"
        );
    }

    // ---- transactional lowering (WS-B) ---------------------------------
    //
    // Each case below is one mistake followed by a correct element that reuses
    // the resources the failed element had already claimed. Exactly one
    // diagnostic means the failed element's claims were rolled back; without
    // the journal the second element reports phantom conflicts of its own.

    #[test]
    fn a_failed_i2c_bus_leaves_its_pins_free() {
        let src = I2C.replace(
            "  <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" frequency=\"100kHz\" />\n",
            "  <i2c id=\"i2c0\" sda=\"6\" scl=\"6\" frequency=\"100kHz\" />\n\
             \x20 <i2c id=\"i2c1\" sda=\"6\" scl=\"7\" frequency=\"100kHz\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(
            errors.len(),
            1,
            "one wiring mistake, one diagnostic: {errors:#?}"
        );
        assert_eq!(errors[0].code, codes::CONFLICT_I2C_WIRING);
    }

    #[test]
    fn failed_bus_lowering_rolls_back_pins() {
        let src = "# espforge\n\
            \x20 <project name=\"spi\" />\n\
            \x20 <chip type=\"esp32c3\" />\n\
            \x20 <runtime type=\"blocking\" />\n\
            # peripherals\n\
            \x20 <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"9\" mode=\"0\" />\n\
            \x20 <spi id=\"spi2\" sclk=\"10\" mosi=\"7\" miso=\"18\" mode=\"0\" />\n\
            \x20 <spi id=\"spi2\" sclk=\"10\" mosi=\"19\" miso=\"20\" mode=\"0\" />\n\
            # devices\n\
            # services\n";
        let errors = lower_src(src).unwrap_err();
        // The second bus claimed SCK 10 before MOSI 7 clashed; the third bus
        // on 10 must not report that leftover claim.
        assert_eq!(errors.len(), 1, "{errors:#?}");
        // Two `<spi>` nodes share the id text but not its span, so this is the
        // ordinary pin conflict rather than the sclk/mosi self-clash wording.
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(
            errors[0].labels[0].text,
            "<spi id=\"spi2\"> and <spi id=\"spi2\"> both use GPIO7"
        );
    }

    #[test]
    fn failed_uart_lowering_rolls_back_its_peripheral_and_pins() {
        let src = "# espforge\n\
            \x20 <project name=\"uart\" />\n\
            \x20 <chip type=\"esp32c3\" />\n\
            \x20 <runtime type=\"blocking\" />\n\
            # peripherals\n\
            \x20 <uart id=\"uart1\" uart=\"UART1\" tx=\"6\" rx=\"5\" />\n\
            \x20 <uart id=\"other\" uart=\"UART2\" tx=\"7\" rx=\"5\" />\n\
            \x20 <uart id=\"third\" uart=\"UART2\" tx=\"7\" rx=\"8\" />\n\
            # devices\n\
            # services\n";
        let errors = lower_src(src).unwrap_err();
        // `other` claimed UART2 and TX 7 before RX 5 clashed: `third` gets both
        // back, so the only problem left is the RX conflict itself.
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(
            errors[0].labels[0].text,
            "<uart id=\"uart1\"> and <uart id=\"other\"> both use GPIO5"
        );
    }

    #[test]
    fn failed_device_lowering_rolls_back_its_aux_pins() {
        let src = "# espforge\n\
            \x20 <project name=\"tft\" />\n\
            \x20 <chip type=\"esp32c3\" />\n\
            \x20 <runtime type=\"blocking\" />\n\
            # peripherals\n\
            \x20 <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" mode=\"0\" />\n\
            \x20 <gpio id=\"gpio4\" pin=\"4\" direction=\"output\" />\n\
            \x20 <gpio id=\"gpio5\" pin=\"5\" direction=\"output\" />\n\
            \x20 <gpio id=\"gpio6\" pin=\"6\" direction=\"output\" />\n\
            \x20 <gpio id=\"gpio20\" pin=\"20\" direction=\"output\" />\n\
            # devices\n\
            \x20 <ili9341 id=\"tft\" bus=\"$spi2\" cs=\"9\" dc=\"$gpio4\" reset=\"$gpio5\" />\n\
            \x20 <ili9341 id=\"lcd\" bus=\"$spi2\" cs=\"10\" dc=\"$gpio6\" reset=\"$gpio5\" />\n\
            \x20 <ili9341 id=\"third\" bus=\"$spi2\" cs=\"18\" dc=\"$gpio6\" reset=\"$gpio20\" />\n\
            # services\n";
        let errors = lower_src(src).unwrap_err();
        // `lcd` claimed DC 6 before its RESET clashed with `tft`; `third` can
        // use DC 6, so the display after the broken one is not blamed too.
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::CONFLICT_PIN);
        assert_eq!(
            errors[0].labels[0].text,
            "<ili9341 id=\"tft\"> and <ili9341 id=\"lcd\"> both use GPIO5"
        );
    }

    const UART_PROBE_BLOCKING: &str = r##"
        driver "uart_probe" {
            format_version 2
            summary "test"
            category "sensor"
            bus "uart"
            codegen {
                feature "uart-probe"
                runtime "blocking" {
                    type "test::UartProbe"
                    constructor "test::UartProbe::new({bus_device})"
                }
            }
            example "minimal" #"<uart_probe id="probe" bus="$uart1" />"#
        }
    "##;

    const UART_PROBE_ASYNC: &str = r##"
        driver "uart_probe" {
            format_version 2
            summary "test"
            category "sensor"
            bus "uart"
            codegen {
                feature "uart-probe"
                runtime "async" {
                    type "test::UartProbe"
                    constructor "test::UartProbe::new({bus_device})"
                }
            }
            attr "init" kind="choice" default="boot" {
                choices "boot" "deferred"
            }
            example "minimal" #"<uart_probe id="probe" bus="$uart1" />"#
        }
    "##;

    /// Lower one UART probe project against a probe manifest.
    fn lower_uart_probe(manifest: &str, devices: &str) -> Result<Board, Vec<Diagnostic>> {
        let session =
            Session::compose(&[("uart-probe.schema.kdl".to_string(), manifest.to_string())])
                .expect("the probe schema loads");
        let src = format!(
            "# espforge\n\
             \x20 <project name=\"probe\" />\n\
             \x20 <chip type=\"esp32c3\" />\n\
             \x20 <runtime type=\"blocking\" />\n\
             # peripherals\n\
             \x20 <uart id=\"uart1\" tx=\"6\" rx=\"5\" />\n\
             # devices\n\
             {devices}\
             # services\n"
        );
        let parsed = parse_espf(&src);
        let section = |name: &str| parsed.sections.get(name).map(Vec::as_slice).unwrap_or(&[]);
        lower(
            meta(),
            section("peripherals"),
            section("devices"),
            section("services"),
            &session,
        )
    }

    /// A `bus "uart"` recipe becomes a device that owns the port: the transport
    /// names the port, and emission builds the `UartDevice` for it.
    #[test]
    fn a_uart_device_recipe_binds_the_port() {
        let board = lower_uart_probe(
            UART_PROBE_BLOCKING,
            "  <uart_probe id=\"probe\" bus=\"$uart1\" />\n",
        )
        .expect("the probe binds uart1");

        assert_eq!(board.devices.len(), 1);
        let device = board.devices[0].as_bus().expect("a bus device");
        assert_eq!(device.tag, "uart_probe");
        assert!(
            matches!(device.transport, BusTransport::Uart(_)),
            "{:#?}",
            device.transport
        );
        assert!(
            matches!(board.resources.bus(device.bus()), Some(Bus::Uart { id, .. }) if id.raw == "uart1"),
            "the device binds the declared port"
        );
    }

    /// A UART port has one peer: a second device on it would build the
    /// peripheral twice, so lowering refuses it and names both parts.
    #[test]
    fn two_devices_cannot_drive_one_uart_port() {
        let errors = lower_uart_probe(
            UART_PROBE_BLOCKING,
            "  <uart_probe id=\"probe\" bus=\"$uart1\" />\n\
             \x20 <uart_probe id=\"second\" bus=\"$uart1\" />\n",
        )
        .unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(errors[0].message, "the uart port is already driven");
        assert_eq!(
            errors[0].labels[0].text,
            "<uart_probe id=\"second\"> binds to `uart1`, which <uart_probe id=\"probe\"> already drives"
        );
    }

    /// `espforge-runtime` compiles one `UartDevice` flavour per build, so a
    /// driver whose runtime does not match the project's is refused rather
    /// than emitted against a type the crate does not have.
    #[test]
    fn an_async_uart_driver_needs_an_embassy_project() {
        let errors = lower_uart_probe(
            UART_PROBE_ASYNC,
            "  <uart_probe id=\"probe\" bus=\"$uart1\" init=\"deferred\" />\n",
        )
        .unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::DEVICE_INVALID);
        assert_eq!(
            errors[0].message,
            "the uart driver does not match the project runtime"
        );
        assert_eq!(
            errors[0].labels[0].text,
            "<uart_probe id=\"probe\"> is an async driver, but this project runs on a blocking \
             runtime; espforge-runtime compiles one UART flavour per build"
        );
    }

    /// A manifest-level input line: the declaration becomes an `Input`
    /// constructor argument and the pin is claimed as an input.
    #[test]
    fn an_input_pin_reaches_the_constructor_as_an_input() {
        use crate::devices::recipe::DeviceRecipeElement;

        const PIN_PROBE: &str = r##"
            driver "pin_probe" {
                format_version 2
                summary "test"
                category "sensor"
                dependency "embedded-graphics" { version "0.8.1" }
                bus "i2c" { address fixed="0x10" }
                pin "dc" direction="output" default_level="low"
                pin "ready" direction="input" pull="up"
                codegen {
                    feature "pin-probe"
                    runtime "blocking" {
                        type "test::PinProbe"
                        constructor "test::PinProbe::new({bus_device}, {address}, {dc_pin}, {ready})"
                    }
                }
                example "minimal" #"<pin_probe id="probe" bus="$i2c0" address="0x10" dc="$gpio4" ready="$ready" />"#
            }
        "##;
        let session =
            Session::compose(&[("pin-probe.schema.kdl".to_string(), PIN_PROBE.to_string())])
                .expect("the probe schema loads");

        let src = "# espforge\n\
            \x20 <project name=\"probe\" />\n\
            \x20 <chip type=\"esp32c3\" />\n\
            \x20 <runtime type=\"blocking\" />\n\
            # peripherals\n\
            \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n\
            \x20 <gpio id=\"gpio4\" pin=\"4\" />\n\
            \x20 <gpio id=\"ready\" pin=\"5\" direction=\"input\" />\n\
            # devices\n\
            \x20 <pin_probe id=\"probe\" bus=\"$i2c0\" address=\"0x10\" dc=\"$gpio4\" ready=\"$ready\" />\n\
            # services\n";
        let parsed = parse_espf(src);
        let peripherals = parsed
            .sections
            .get("peripherals")
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let devices = parsed
            .sections
            .get("devices")
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let mut cx = LowerCx::with_schema(session.schema());
        cx.set_meta(&meta());
        for (node, tag) in peripherals.iter().zip(["i2c", "gpio", "gpio"]) {
            session
                .element(tag)
                .unwrap_or_else(|| panic!("`<{tag}>` is implemented"))
                .lower(node, &mut cx)
                .unwrap_or_else(|error| panic!("`<{tag}>` lowers: {error:?}"));
        }
        DeviceRecipeElement::new("pin_probe")
            .lower(&devices[0], &mut cx)
            .expect("the probe lowers");

        let board = cx.into_board(meta()).expect("the board is consistent");
        let device = board.devices[0].as_bus().expect("a bus device");
        let ready = device
            .driver
            .args
            .iter()
            .map(|arg| arg.text())
            .find(|text| text.contains("Input::new"))
            .expect("the input line is a constructor argument");
        assert!(ready.contains("esp_hal::gpio::Pull::Up"), "{ready}");
        assert!(
            device.driver.pins.iter().any(|(name, _)| name == "ready"),
            "the input line is claimed"
        );
    }

    /// A manifest-level bidirectional line: it becomes the flex wrapper as a
    /// constructor argument and the pin is claimed for a line that both drives
    /// and reads.
    #[test]
    fn a_bidirectional_pin_reaches_the_constructor_as_a_flex() {
        use crate::devices::recipe::DeviceRecipeElement;

        const FLEX_PROBE: &str = r##"
            driver "flex_probe" {
                format_version 2
                summary "test"
                category "sensor"
                dependency "embedded-graphics" { version "0.8.1" }
                bus "i2c" { address fixed="0x10" }
                pin "line" direction="bidirectional" pull="up"
                codegen {
                    feature "flex-probe"
                    runtime "blocking" {
                        type "test::FlexProbe"
                        constructor "test::FlexProbe::new({bus_device}, {address}, {line})"
                    }
                }
                example "minimal" #"<flex_probe id="probe" bus="$i2c0" address="0x10" line="$line" />"#
            }
        "##;
        let session =
            Session::compose(&[("flex-probe.schema.kdl".to_string(), FLEX_PROBE.to_string())])
                .expect("the probe schema loads");

        let src = "# espforge\n\
            \x20 <project name=\"probe\" />\n\
            \x20 <chip type=\"esp32c3\" />\n\
            \x20 <runtime type=\"blocking\" />\n\
            # peripherals\n\
            \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n\
            \x20 <gpio id=\"line\" pin=\"5\" direction=\"bidirectional\" />\n\
            # devices\n\
            \x20 <flex_probe id=\"probe\" bus=\"$i2c0\" address=\"0x10\" line=\"$line\" />\n\
            # services\n";
        let parsed = parse_espf(src);
        let peripherals = parsed
            .sections
            .get("peripherals")
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let devices = parsed
            .sections
            .get("devices")
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let mut cx = LowerCx::with_schema(session.schema());
        cx.set_meta(&meta());
        for (node, tag) in peripherals.iter().zip(["i2c", "gpio"]) {
            session
                .element(tag)
                .unwrap_or_else(|| panic!("`<{tag}>` is implemented"))
                .lower(node, &mut cx)
                .unwrap_or_else(|error| panic!("`<{tag}>` lowers: {error:?}"));
        }
        DeviceRecipeElement::new("flex_probe")
            .lower(&devices[0], &mut cx)
            .expect("the probe lowers");

        let board = cx.into_board(meta()).expect("the board is consistent");
        let device = board.devices[0].as_bus().expect("a bus device");
        let line = device
            .driver
            .args
            .iter()
            .map(|arg| arg.text())
            .find(|text| text.contains("Bidirectional::new"))
            .expect("the switching line is a constructor argument");
        assert!(line.contains("esp_hal::gpio::Pull::Up"), "{line}");
        let (_, claim) = device
            .driver
            .pins
            .iter()
            .find(|(name, _)| name == "line")
            .expect("the switching line is claimed");
        assert_eq!(claim.claim().need, PinUse::Bidirectional);
    }

    /// A bidirectional line is not an input: a button on one is refused with
    /// the third spelling in the label.
    #[test]
    fn a_button_cannot_bind_a_bidirectional_line() {
        let src = BUTTON.replace("direction=\"input\"", "direction=\"bidirectional\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::COMPONENT_WRONG_DIRECTION);
        assert_eq!(
            errors[0].labels[0].text,
            "<button id=\"button\"> needs an input pin, but `gpio9` has direction=\"bidirectional\""
        );
    }

    // ---- reserved identifiers (WS-C) ------------------------------------

    #[test]
    fn a_device_id_cannot_shadow_a_generated_binding_of_a_pin_element() {
        let src = BLINK.replace("id=\"red_led\"", "id=\"devices\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::DEVICE_RESERVED_ID);
        assert_eq!(errors[0].message, "reserved device id");
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"devices\">: `devices` is a name the generated code binds; rename the id"
        );
        // The underline is the id value, so the fix is a rename of that text.
        assert_eq!(&src[errors[0].primary.clone()], "devices");
    }

    #[test]
    fn every_reserved_name_is_refused() {
        for reserved in ["devices", "ctx", "logger", "spawner", "setup", "Self"] {
            let src = BLINK.replace("id=\"red_led\"", &format!("id=\"{reserved}\""));
            let errors = lower_src(&src).unwrap_err();
            assert_eq!(errors.len(), 1, "`{reserved}`: {errors:#?}");
            assert_eq!(errors[0].code, codes::DEVICE_RESERVED_ID, "`{reserved}`");
        }
    }

    #[test]
    fn a_double_underscore_prefix_is_reserved_for_storage() {
        let src = BLINK.replace("id=\"red_led\"", "id=\"__red_led\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_RESERVED_ID);
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"__red_led\">: ids starting with `__` are reserved for the generated storage; rename the id"
        );
    }

    #[test]
    fn a_rust_keyword_is_refused_and_near_misses_are_not() {
        let src = BLINK.replace("id=\"red_led\"", "id=\"type\"");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_RESERVED_ID);
        assert_eq!(
            errors[0].labels[0].text,
            "<led id=\"type\">: `type` is a Rust keyword; rename the id"
        );

        for ok in ["device_name", "devices2", "my_ctx", "spin"] {
            let src = BLINK.replace("id=\"red_led\"", &format!("id=\"{ok}\""));
            lower_src(&src).unwrap_or_else(|errors| panic!("`{ok}` is a fine id: {errors:#?}"));
        }
    }

    #[test]
    fn a_peripheral_id_cannot_shadow_a_generated_binding() {
        let src = BLINK
            .replace("id=\"gpio18\"", "id=\"registry\"")
            .replace("$gpio18", "$registry");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::COMPONENT_RESERVED_ID);
        assert_eq!(errors[0].message, "reserved peripheral id");
    }

    #[test]
    fn a_device_id_cannot_shadow_a_generated_binding() {
        let errors = lower_src(&TFT.replace("id=\"tft\"", "id=\"devices\"")).unwrap_err();
        assert_eq!(errors[0].code, codes::DEVICE_RESERVED_ID);
        assert_eq!(errors[0].message, "reserved device id");
        assert_eq!(
            errors[0].labels[0].text,
            "<ili9341 id=\"devices\">: `devices` is a name the generated code binds; rename the id"
        );
    }

    #[test]
    fn a_service_id_cannot_shadow_a_generated_binding() {
        let src = with_service("<http_client id=\"services\" network=\"$home\" />");
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors[0].code, codes::SERVICE_RESERVED_ID);
        assert_eq!(errors[0].message, "reserved service id");
        assert_eq!(
            errors[0].labels[0].text,
            "<http_client id=\"services\">: `services` is a name the generated code binds; rename the id"
        );
    }

    #[test]
    fn a_reserved_id_does_not_hide_the_other_errors() {
        // One bad id and one bad reference in different elements: both are
        // reported, so a rename fixes one thing at a time.
        let src = BLINK.replace("id=\"red_led\"", "id=\"ctx\"").replace(
            "  <led id=\"ctx\" gpio=\"$gpio18\" />\n",
            "  <led id=\"ctx\" gpio=\"$gpio18\" />\n  <button id=\"press\" gpio=\"$gpio99\" />\n",
        );
        let errors = lower_src(&src).unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:#?}");
        let found: Vec<_> = errors.iter().map(|error| error.code).collect();
        assert!(found.contains(&codes::DEVICE_RESERVED_ID), "{found:?}");
        assert!(found.contains(&codes::COMPONENT_UNKNOWN_GPIO), "{found:?}");
    }

    // ---- WS-D: the chip's pin table ----

    /// One `<gpio>` and one `<led>` that claims it, so the pin rules are the
    /// only thing a test has to change.
    const LED_ON_PIN: &str = "# espforge\n\
        \x20 <project name=\"pins\" />\n\
        \x20 <chip type=\"esp32\" />\n\
        \x20 <runtime type=\"blocking\" />\n\
        # peripherals\n\
        \x20 <gpio id=\"gpio34\" pin=\"34\" />\n\
        # devices\n\
        \x20 <led id=\"red_led\" gpio=\"$gpio34\" />\n\
        # services\n";

    #[test]
    fn an_output_on_an_input_only_pin_is_refused() {
        let errors = lower_on("esp32", LED_ON_PIN).unwrap_err();
        assert_eq!(errors[0].code, codes::GPIO_NOT_OUTPUT_CAPABLE);
        assert_eq!(errors[0].message, "GPIO34 is input-only on esp32");
        assert_eq!(
            errors[0].labels[0].text,
            "drive this pin as an input, or pick an output-capable GPIO"
        );
    }

    /// The refusal happens before the graph records the claim, so a second
    /// element on the same pin sees a free pin, not a conflict.
    #[test]
    fn a_refused_pin_leaves_no_claim_behind() {
        let src = LED_ON_PIN.replace(
            "  <led id=\"red_led\" gpio=\"$gpio34\" />\n",
            "  <led id=\"red_led\" gpio=\"$gpio34\" />\n  <button id=\"press\" gpio=\"$gpio34\" />\n",
        );
        let errors = lower_on("esp32", &src).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].code, codes::GPIO_NOT_OUTPUT_CAPABLE);
    }

    #[test]
    fn an_input_on_an_input_only_pin_lowers() {
        let src = LED_ON_PIN.replace(
            "<led id=\"red_led\" gpio=\"$gpio34\" />",
            "<button id=\"press\" gpio=\"$gpio34\" />",
        );
        assert!(lower_on("esp32", &src).is_ok());
    }

    /// The S3's flash is GPIO26..GPIO37. The plan's illustration ("GPIO6 on
    /// an S3") is the classic ESP32's flash range, and the examples put a
    /// display on GPIO6/7 on an S3, so this pins the real boundary.
    #[test]
    fn a_flash_pin_is_refused_on_an_s3() {
        let src = LED_ON_PIN
            .replace("<chip type=\"esp32\" />", "<chip type=\"esp32s3\" />")
            .replace("pin=\"34\"", "pin=\"26\"")
            .replace("gpio34", "gpio26");
        let errors = lower_on("esp32s3", &src).unwrap_err();
        assert_eq!(errors[0].code, codes::GPIO_RESERVED_FLASH);
        assert_eq!(
            errors[0].message,
            "GPIO26 is reserved for SPI flash on esp32s3"
        );
    }

    #[test]
    fn an_s3_led_on_gpio6_is_fine() {
        let src = LED_ON_PIN
            .replace("<chip type=\"esp32\" />", "<chip type=\"esp32s3\" />")
            .replace("pin=\"34\"", "pin=\"6\"")
            .replace("gpio34", "gpio6");
        assert!(lower_on("esp32s3", &src).is_ok());
    }

    #[test]
    fn a_pin_the_chip_does_not_have_is_refused() {
        let src = LED_ON_PIN
            .replace("pin=\"34\"", "pin=\"99\"")
            .replace("gpio34", "gpio99");
        let errors = lower_on("esp32", &src).unwrap_err();
        assert_eq!(errors[0].code, codes::GPIO_NOT_ON_CHIP);
        assert_eq!(errors[0].message, "GPIO99 does not exist on esp32");
        assert_eq!(errors[0].labels[0].text, "this chip has GPIO0..GPIO39");
    }

    /// The button example's input sits on GPIO9, the c3's boot pin: a warning,
    /// never a failure.
    #[test]
    fn a_strapping_pin_warns_and_still_lowers() {
        let board = lower_src(BUTTON).expect("a warning does not stop lowering");
        let strapping: Vec<_> = board
            .warnings
            .iter()
            .filter(|warning| warning.code == codes::GPIO_STRAPPING)
            .collect();
        assert_eq!(strapping.len(), 1, "{:#?}", board.warnings);
        assert_eq!(strapping[0].message, "GPIO9 is a strapping pin on esp32c3");
    }

    /// A chip espforge has no table for is a hard error, never a silent skip.
    /// `lower_src_with` bypasses the core reader, which is what lets a name
    /// outside the catalog reach the pin check.
    #[test]
    fn a_chip_without_a_table_is_a_hard_error() {
        let mut meta = meta();
        meta.chip = "nonesuch".into();
        let errors = lower_src_with(BLINK, meta).unwrap_err();
        assert_eq!(errors[0].code, codes::PIN_CAPS_MISSING);
    }
}

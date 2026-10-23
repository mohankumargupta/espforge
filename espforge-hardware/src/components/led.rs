//! `<led>` component lowering.
//!
//! `<led>` is a component over `<gpio>` (like `i2c_component` over `<i2c>`):
//! the `$gpio` parent lookup goes through the typed wrapper
//! (`LedComponent::resolve`), the sole consumer of the context handles.
//! Pin/direction/duplicate checks stay here so messages stay byte-identical.
//! The `<gpio>` table itself lives in [`crate::peripherals::gpio`].

use crate::peripherals::gpio::{gpio_for_handle, GpioEntry};
use crate::{ButtonSpec, LedSpec, is_rust_ident};
use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_bindings::components::LedComponent;

/// The `<led>` branch of the `# components` loop. `buttons` holds the
/// buttons lowered so far (for the input/output clash check).
pub fn attach_led(
    n: &ParsedNode,
    gpios: &[GpioEntry],
    leds: &mut Vec<LedSpec>,
    buttons: &[ButtonSpec],
    ctx: &RuntimeContext,
) -> Result<(), EspfError> {
    // Single dispatch path for the `$` ref; wrapper error text already matches
    // hardware's "not a <gpio>" / active_low / slug messages.
    let comp =
        LedComponent::resolve(n, ctx).map_err(|m| EspfError::at_node(n, "invalid led", m))?;
    let gpio = gpio_for_handle(gpios, ctx, comp.peripheral_handle, n, comp.gpio_ref)?;
    let (pin, direction) = (gpio.pin, gpio.direction.as_deref());
    if matches!(direction, Some(d) if d != "output") {
        return Err(EspfError::at_node(
            n,
            "wrong gpio direction",
            format!(
                "<led id=\"{}\"> needs an output pin, but `{}` has direction=\"{}\"",
                n.id,
                gpio.id,
                direction.unwrap_or("")
            ),
        ));
    }
    let active_low = match n.get_attr("active_low") {
        None | Some("false") => false,
        Some("true") => true,
        Some(other) => {
            return Err(EspfError::at_node(
                n,
                "invalid active_low",
                format!(
                    "<led id=\"{}\"> active_low=\"{other}\" must be \"true\" or \"false\"",
                    n.id
                ),
            ));
        }
    };
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(EspfError::at_node(
            n,
            "invalid component id",
            format!(
                "<led id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ),
        ));
    }
    if let Some(other) = leds.iter().find(|l| l.pin == pin) {
        return Err(EspfError::at_node(
            n,
            "pin already used",
            format!(
                "<led id=\"{}\"> and <led id=\"{}\"> both use GPIO{pin}",
                other.name, n.id
            ),
        ));
    }
    if let Some(b) = buttons.iter().find(|b| b.pin == pin) {
        return Err(EspfError::at_node(
            n,
            "pin direction clash",
            format!(
                "<button id=\"{}\"> and <led id=\"{}\">: one pin cannot be both input and output",
                b.name, n.id
            ),
        ));
    }
    leds.push(LedSpec {
        name,
        pin,
        active_low,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::lower_sections;
    use super::*;

    const WITH_LED: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" active_low=\"true\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<crate::Hardware, String> {
        lower_sections(&sections(src))
    }

    #[test]
    fn gpio_with_a_led_is_buildable_and_becomes_hardware() {
        let hw = lower_src(WITH_LED).unwrap();
        assert_eq!(
            hw.leds,
            [LedSpec {
                name: "red_led".into(),
                pin: 18,
                active_low: true
            }]
        );
    }

    #[test]
    fn two_leds_on_one_pin_are_refused() {
        let src = WITH_LED.replace(
            "# devices",
            "  <led id=\"other\" gpio=\"$gpio18\" />\n# devices",
        );
        assert!(lower_src(&src).unwrap_err().contains("both use GPIO18"));
    }

    #[test]
    fn a_led_on_an_input_pin_is_refused() {
        let src = WITH_LED.replace("output", "input");
        assert!(lower_src(&src).unwrap_err().contains("needs an output pin"));
    }

    #[test]
    fn a_led_and_a_button_cannot_share_one_pin() {
        // A led needs an output gpio and a button an input one, so sharing a
        // physical pin uses two `<gpio>` entries with the same pin number.
        for (first, second) in [
            (
                "  <led id=\"red_led\" gpio=\"$gpio9\" />\n  <button id=\"button\" gpio=\"$gpio9b\" />",
                "led before button",
            ),
            (
                "  <button id=\"button\" gpio=\"$gpio9b\" />\n  <led id=\"red_led\" gpio=\"$gpio9\" />",
                "button before led",
            ),
        ] {
            let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />\n  <gpio id=\"gpio9b\" pin=\"9\" direction=\"input\" />\n# components\n"
                .to_string()
                + first
                + "\n# devices\n# services\n";
            let err = lower_src(&src).unwrap_err();
            assert!(err.contains("input and output"), "{second}: {err}");
        }
    }
}

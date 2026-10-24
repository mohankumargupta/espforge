//! `<button>` component lowering.
//!
//! `<button>` is a component over `<gpio>` (like `i2c_component` over
//! `<i2c>`): the `$gpio` parent lookup goes through the typed wrapper
//! (`ButtonComponent::resolve`), the sole consumer of the context handles.
//! Pin/direction/duplicate checks stay here so messages stay byte-identical.
//! The `<gpio>` table itself lives in [`crate::peripherals::gpio`].

use crate::peripherals::gpio::{GpioEntry, gpio_for_handle};
use crate::{ButtonSpec, LedSpec, is_rust_ident};
use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_bindings::components::ButtonComponent;

/// The `<button>` branch of the `# components` loop. `leds` is the leds
/// lowered so far (for the input/output clash check).
pub fn attach_button(
    n: &ParsedNode,
    gpios: &[GpioEntry],
    leds: &[LedSpec],
    buttons: &mut Vec<ButtonSpec>,
    ctx: &RuntimeContext,
) -> Result<(), EspfError> {
    // Single dispatch path for the `$` ref; wrapper error text already matches
    // hardware's "not a <gpio>" / pull_up / slug messages.
    let comp =
        ButtonComponent::resolve(n, ctx).map_err(|m| EspfError::at_node(n, "invalid button", m))?;
    let gpio = gpio_for_handle(gpios, ctx, comp.peripheral_handle, n, comp.gpio_ref)?;
    let (pin, direction) = (gpio.pin, gpio.direction.as_deref());
    if matches!(direction, Some(d) if d != "input") {
        return Err(EspfError::at_node(
            n,
            "wrong gpio direction",
            format!(
                "<button id=\"{}\"> needs an input pin, but `{}` has direction=\"{}\"",
                n.id,
                gpio.id,
                direction.unwrap_or("")
            ),
        ));
    }
    // The wrapper coerced `pull_up` from the schema and already rejected a
    // non-boolean value; hardware only needs the value.
    let pull_up = comp.pull_up;
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(EspfError::at_node(
            n,
            "invalid component id",
            format!(
                "<button id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ),
        ));
    }
    if let Some(other) = buttons.iter().find(|b| b.pin == pin) {
        return Err(EspfError::at_node(
            n,
            "pin already used",
            format!(
                "<button id=\"{}\"> and <button id=\"{}\"> both use GPIO{pin}",
                other.name, n.id
            ),
        ));
    }
    if leds.iter().any(|l| l.pin == pin) {
        return Err(EspfError::at_node(
            n,
            "pin direction clash",
            format!(
                "<led id=\"{}\"> and <button id=\"{}\">: one pin cannot be both input and output",
                n.id, name
            ),
        ));
    }
    buttons.push(ButtonSpec { name, pin, pull_up });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower_sections;

    const WITH_BUTTON: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n# components\n  <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<crate::Hardware, String> {
        lower_sections(&sections(src))
    }

    #[test]
    fn a_button_is_buildable_and_becomes_hardware() {
        let hw = lower_src(WITH_BUTTON).unwrap();
        assert_eq!(
            hw.buttons,
            [ButtonSpec {
                name: "button".into(),
                pin: 9,
                pull_up: true
            }]
        );
    }

    #[test]
    fn pull_up_defaults_to_false_and_bad_values_are_refused() {
        let src = WITH_BUTTON.replace(" pull_up=\"true\"", "");
        assert!(!lower_src(&src).unwrap().buttons[0].pull_up);

        let bad = WITH_BUTTON.replace("pull_up=\"true\"", "pull_up=\"up\"");
        assert!(
            lower_src(&bad)
                .unwrap_err()
                .contains("must be \"true\" or \"false\"")
        );
    }

    #[test]
    fn a_button_refers_to_a_gpio_peripheral() {
        // Referring at something that exists but is not a <gpio>: the
        // component then rejects it.
        let src = WITH_BUTTON
            .replace(
                "# peripherals",
                "# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />",
            )
            .replace("$gpio9", "$i2c0");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("which is not a <gpio>"), "{err}");
    }

    #[test]
    fn a_button_on_an_output_pin_is_refused() {
        let src = WITH_BUTTON.replace("input", "output");
        assert!(lower_src(&src).unwrap_err().contains("needs an input pin"));
    }

    #[test]
    fn two_buttons_on_one_pin_are_refused() {
        let src = WITH_BUTTON.replace(
            "# devices",
            "  <button id=\"other\" gpio=\"$gpio9\" />\n# devices",
        );
        assert!(lower_src(&src).unwrap_err().contains("both use GPIO9"));
    }
}

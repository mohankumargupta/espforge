//! `<gpio>` table + `<led>` / `<button>` lowering (step 1d).
//!
//! Moved verbatim from `espforge/src/commands/pipeline.rs::hardware`: the
//! `<gpio>` peripheral branch and the `<led>` / `<button>` component
//! branches (pin resolution, direction checks, duplicate-pin errors).

use super::{ButtonSpec, LedSpec, is_rust_ident};
use espf::context::ParsedNode;

/// One `<gpio>` peripheral: id -> pin + optional direction.
#[derive(Debug, Clone)]
pub struct GpioEntry {
    pub id: String,
    pub pin: u8,
    pub direction: Option<String>,
}

/// The `<gpio>` branch of the `# peripherals` loop.
pub fn lower_gpio(n: &ParsedNode) -> Result<GpioEntry, String> {
    let pin = n
        .get_attr("pin")
        .and_then(|p| p.parse::<u8>().ok())
        .ok_or_else(|| format!("<gpio id=\"{}\"> needs a numeric pin=\"...\"", n.id))?;
    Ok(GpioEntry {
        id: n.id.to_string(),
        pin,
        direction: n.get_attr("direction").map(str::to_string),
    })
}

/// Look a `$gpio` ref up in the gpio table. Kept as a helper so the
/// `<led>` / `<button>` branches share the "not a `<gpio>`" message.
pub fn resolve_gpio<'a>(
    ref_raw: &str,
    owner_tag: &str,
    owner_id: &str,
    gpios: &'a [GpioEntry],
) -> Result<&'a GpioEntry, String> {
    let target = ref_raw.trim_start_matches('$');
    gpios.iter().find(|g| g.id == target).ok_or_else(|| {
        format!(
            "<{owner_tag} id=\"{owner_id}\"> refers to `{target}`, which is not a <gpio> in `# peripherals`"
        )
    })
}

/// The `<button>` branch of the `# components` loop. `leds` is the leds
/// lowered so far (for the input/output clash check).
pub fn attach_button(
    n: &ParsedNode,
    gpios: &[GpioEntry],
    leds: &[LedSpec],
    buttons: &mut Vec<ButtonSpec>,
) -> Result<(), String> {
    let gpio_ref = n
        .get_attr("gpio")
        .ok_or_else(|| format!("<button id=\"{}\"> needs gpio=\"$<gpio id>\"", n.id))?;
    let gpio = resolve_gpio(gpio_ref, "button", n.id, gpios)?;
    let (pin, direction) = (gpio.pin, gpio.direction.as_deref());
    if matches!(direction, Some(d) if d != "input") {
        return Err(format!(
            "<button id=\"{}\"> needs an input pin, but `{}` has direction=\"{}\"",
            n.id,
            gpio.id,
            direction.unwrap_or("")
        ));
    }
    let pull_up = match n.get_attr("pull_up") {
        None | Some("false") => false,
        Some("true") => true,
        Some(other) => {
            return Err(format!(
                "<button id=\"{}\"> pull_up=\"{other}\" must be \"true\" or \"false\"",
                n.id
            ));
        }
    };
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(format!(
            "<button id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
            n.id
        ));
    }
    if let Some(other) = buttons.iter().find(|b| b.pin == pin) {
        return Err(format!(
            "<button id=\"{}\"> and <button id=\"{}\"> both use GPIO{pin}",
            other.name, n.id
        ));
    }
    if leds.iter().any(|l| l.pin == pin) {
        return Err(format!(
            "<led id=\"{}\"> and <button id=\"{}\">: one pin cannot be both input and output",
            n.id, name
        ));
    }
    buttons.push(ButtonSpec {
        name,
        pin,
        pull_up,
    });
    Ok(())
}

/// The `<led>` branch of the `# components` loop. `buttons` holds the
/// buttons lowered so far (for the input/output clash check).
pub fn attach_led(
    n: &ParsedNode,
    gpios: &[GpioEntry],
    leds: &mut Vec<LedSpec>,
    buttons: &[ButtonSpec],
) -> Result<(), String> {
    let gpio_ref = n
        .get_attr("gpio")
        .ok_or_else(|| format!("<led id=\"{}\"> needs gpio=\"$<gpio id>\"", n.id))?;
    let gpio = resolve_gpio(gpio_ref, "led", n.id, gpios)?;
    let (pin, direction) = (gpio.pin, gpio.direction.as_deref());
    if matches!(direction, Some(d) if d != "output") {
        return Err(format!(
            "<led id=\"{}\"> needs an output pin, but `{}` has direction=\"{}\"",
            n.id,
            gpio.id,
            direction.unwrap_or("")
        ));
    }
    let active_low = match n.get_attr("active_low") {
        None | Some("false") => false,
        Some("true") => true,
        Some(other) => {
            return Err(format!(
                "<led id=\"{}\"> active_low=\"{other}\" must be \"true\" or \"false\"",
                n.id
            ));
        }
    };
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(format!(
            "<led id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
            n.id
        ));
    }
    if let Some(other) = leds.iter().find(|l| l.pin == pin) {
        return Err(format!(
            "<led id=\"{}\"> and <led id=\"{}\"> both use GPIO{pin}",
            other.name, n.id
        ));
    }
    if let Some(b) = buttons.iter().find(|b| b.pin == pin) {
        return Err(format!(
            "<button id=\"{}\"> and <led id=\"{}\">: one pin cannot be both input and output",
            b.name, n.id
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
    use super::super::lower;
    use super::*;

    const WITH_LED: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" active_low=\"true\" />\n# devices\n# services\n";

    const WITH_BUTTON: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n# components\n  <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<super::super::Hardware, String> {
        lower(&sections(src))
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

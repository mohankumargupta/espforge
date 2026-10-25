//! Example overlay placeholders: a fixed token set, not a template language.
//!
//! An example may ship its own `diagram.json` (and, in principle, its own
//! `wokwi.toml`). Those files were the last thing the template engine did;
//! this module accepts exactly the four forms the examples use and rejects
//! anything else by name:
//!
//! | token | value |
//! |---|---|
//! | `{{ board }}` | the Wokwi board id for the project's chip |
//! | `{{ serialMonitor }}` | `$serialMonitor` |
//! | `{{ <component>.pin }}` | the GPIO number of a led or button |
//!
//! A `{{ ... }}` that is not one of those is an [`OverlayError`], which the CLI
//! reports as a user error rather than an internal one. The same applies to a
//! `.pin` on a bus component, which has no single pin.

use anyhow::Result;
use espforge_model::{Board, Component};
use std::fmt;

use super::chip::Chip;
use super::wokwi;

/// A placeholder an example's file uses that espforge cannot resolve.
#[derive(Debug)]
pub struct OverlayError {
    /// The example file, as the CLI names it (`diagram.json`).
    pub file: String,
    pub message: String,
}

impl OverlayError {
    fn unknown(file: &str, token: &str) -> Self {
        Self {
            file: file.to_string(),
            message: format!(
                "unknown placeholder `{{{{ {token} }}}}` (supported: `{{{{ board }}}}`, \
                 `{{{{ serialMonitor }}}}`, `{{{{ <component>.pin }}}}`)"
            ),
        }
    }

    fn unclosed(file: &str) -> Self {
        Self {
            file: file.to_string(),
            message: "a `{{` is never closed".to_string(),
        }
    }
}

impl fmt::Display for OverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}`: {}", self.file, self.message)
    }
}

impl std::error::Error for OverlayError {}

/// Replace every placeholder in `text`. `file` only names the file in errors.
pub fn render(text: &str, file: &str, board: &Board) -> Result<String> {
    let chip = Chip::resolve(&board.meta.chip)?;
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(OverlayError::unclosed(file).into());
        };
        out.push_str(&resolve_token(after[..end].trim(), file, board, &chip)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve_token(token: &str, file: &str, board: &Board, chip: &Chip) -> Result<String> {
    match token {
        "board" => return Ok(wokwi::board(&chip.name).to_string()),
        "serialMonitor" => return Ok(wokwi::SERIAL_MONITOR.to_string()),
        _ => {}
    }
    let Some(name) = token.strip_suffix(".pin") else {
        return Err(OverlayError::unknown(file, token).into());
    };
    let name = name.trim();
    let Some(component) = board
        .components
        .iter()
        .find(|c| c.id().raw == name || c.id().rust_name() == name)
    else {
        return Err(OverlayError::unknown(file, token).into());
    };
    match component {
        Component::Led(led) => Ok(led.pin.pin().0.to_string()),
        Component::Button(button) => Ok(button.pin.pin().0.to_string()),
        Component::I2c(_) | Component::Spi(_) | Component::Uart(_) => Err(OverlayError {
            file: file.to_string(),
            message: format!(
                "`{name}` is a bus component and has no single `.pin` \
                 (supported: `{{{{ board }}}}`, `{{{{ serialMonitor }}}}`, \
                 `{{{{ <component>.pin }}}}`)"
            ),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{
        Board, Bus, Button, Component, I2cComponent, Ident, Led, Owner, OwnerKind, Pin, PinUse,
        ResourceGraph,
    };

    fn claim(
        resources: &mut ResourceGraph,
        pin: u8,
        kind: OwnerKind,
        id: &str,
        need: PinUse,
    ) -> espforge_model::ClaimedPin {
        let claim = resources
            .claim(Pin(pin), Owner::new(kind, Ident::new(id, 0..0)), 0..0, need)
            .unwrap();
        claim
    }

    /// A led, a button and an i2c bus: enough to exercise every token arm.
    fn board() -> Board {
        let mut resources = ResourceGraph::new();
        let led_pin = claim(
            &mut resources,
            18,
            OwnerKind::Led,
            "red-led",
            PinUse::Output,
        );
        let button_pin = claim(&mut resources, 9, OwnerKind::Button, "btn", PinUse::Input);
        let sda = claim(
            &mut resources,
            6,
            OwnerKind::I2c,
            "i2c0",
            PinUse::Bidirectional,
        );
        let scl = claim(
            &mut resources,
            7,
            OwnerKind::I2c,
            "i2c0",
            PinUse::Bidirectional,
        );
        let bus = resources.add_bus(Bus::I2c {
            id: Ident::new("i2c0", 0..0),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
            used_by: None,
        });
        Board {
            meta: crate::codegen::test_support::meta("esp32c3"),
            resources,
            components: vec![
                Component::Led(Led {
                    id: Ident::new("red-led", 0..0),
                    pin: led_pin,
                    active_low: false,
                }),
                Component::Button(Button {
                    id: Ident::new("btn", 0..0),
                    pin: button_pin,
                    pull_up: true,
                }),
                Component::I2c(I2cComponent {
                    id: Ident::new("i2c0", 0..0),
                    bus,
                    address: Some(0x48),
                }),
            ],
            services: Vec::new(),
        }
    }

    #[test]
    fn the_four_example_tokens_resolve() {
        let board = board();
        assert_eq!(
            render("{{ board }}", "diagram.json", &board).unwrap(),
            "board-esp32-c3-devkitm-1"
        );
        assert_eq!(
            render("{{ serialMonitor }}", "diagram.json", &board).unwrap(),
            "$serialMonitor"
        );
        assert_eq!(
            render("esp:{{ red_led.pin }}", "diagram.json", &board).unwrap(),
            "esp:18"
        );
        // the id as written works too
        assert_eq!(
            render("esp:{{ red-led.pin }}", "diagram.json", &board).unwrap(),
            "esp:18"
        );
        assert_eq!(render("{{btn.pin}}", "diagram.json", &board).unwrap(), "9");
    }

    #[test]
    fn a_whole_diagram_keeps_everything_else_verbatim() {
        let text = "{\n  \"type\": \"{{ board }}\",\n  \"c\": [\"esp:TX\", \"{{ serialMonitor }}:RX\"]\n}\n";
        let out = render(text, "diagram.json", &board()).unwrap();
        assert!(
            out.contains("\"type\": \"board-esp32-c3-devkitm-1\""),
            "{out}"
        );
        assert!(out.contains("\"$serialMonitor:RX\""), "{out}");
        assert!(!out.contains("{{"), "{out}");
    }

    #[test]
    fn an_unknown_token_is_an_error_that_lists_the_supported_set() {
        let err = render("{{ opt.wifi }}", "diagram.json", &board()).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert_eq!(overlay.file, "diagram.json");
        assert!(overlay.message.contains("unknown placeholder"), "{overlay}");
        assert!(overlay.message.contains("{{ board }}"), "{overlay}");
        assert!(
            overlay.message.contains("{{ <component>.pin }}"),
            "{overlay}"
        );
    }

    #[test]
    fn a_pin_on_a_bus_component_says_why_there_is_no_pin() {
        let err = render("{{ i2c0.pin }}", "d.json", &board()).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert!(overlay.message.contains("no single `.pin`"), "{overlay}");
    }

    #[test]
    fn an_unknown_component_attribute_is_an_overlay_error() {
        let err = render("{{ red_led.sda }}", "d.json", &board()).unwrap_err();
        assert!(err.downcast_ref::<OverlayError>().is_some());
    }

    #[test]
    fn an_unclosed_placeholder_is_an_error() {
        let err = render("{{ board", "d.json", &board()).unwrap_err();
        assert!(
            err.downcast_ref::<OverlayError>()
                .expect("typed error")
                .message
                .contains("never closed")
        );
    }

    #[test]
    fn text_without_placeholders_is_unchanged() {
        let text = "{\n  \"version\": 1\n}\n";
        assert_eq!(render(text, "d.json", &board()).unwrap(), text);
    }
}

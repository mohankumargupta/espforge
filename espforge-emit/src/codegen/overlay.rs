//! Example overlay placeholders: a fixed token set, not a template language.
//!
//! An example may ship its own `diagram.json` (and, in principle, its own
//! `wokwi.toml`). This module accepts exactly the forms the examples use and
//! rejects anything else by name:
//!
//! | token | value |
//! |---|---|
//! | `{{ board }}` | the Wokwi board id for the project's chip |
//! | `{{ serialMonitor }}` | `$serialMonitor` |
//! | `{{ chip }}` | the custom chip's name from the example's `chip.json` |
//! | `{{ <device>.pin }}` | a pin device's GPIO number |
//!
//! A `{{ ... }}` that is not one of those is an [`OverlayError`], which the CLI
//! reports as a user error rather than an internal one.

use anyhow::Result;
use espforge_elements::{Board, Lowered};
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
                 `{{{{ serialMonitor }}}}`, `{{{{ chip }}}}`, `{{{{ <device>.pin }}}}`)"
            ),
        }
    }

    fn unclosed(file: &str) -> Self {
        Self {
            file: file.to_string(),
            message: "a `{{` is never closed".to_string(),
        }
    }

    fn missing_chip(file: &str) -> Self {
        Self {
            file: file.to_string(),
            message: "`{{ chip }}` needs a `chip.json` next to it to know the chip's name"
                .to_string(),
        }
    }
}

impl fmt::Display for OverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}`: {}", self.file, self.message)
    }
}

impl std::error::Error for OverlayError {}

/// Replace every placeholder in `text`. `file` only names the file in errors;
/// `chip` is the custom chip's name when the example ships a `chip.json`.
pub fn render(text: &str, file: &str, board: &Board, chip: Option<&str>) -> Result<String> {
    let board_chip = Chip::resolve(&board.meta.chip)?;
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(OverlayError::unclosed(file).into());
        };
        out.push_str(&resolve_token(
            after[..end].trim(),
            file,
            board,
            &board_chip,
            chip,
        )?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve_token(
    token: &str,
    file: &str,
    board: &Board,
    board_chip: &Chip,
    custom_chip: Option<&str>,
) -> Result<String> {
    match token {
        "board" => return Ok(wokwi::board(&board_chip.name).to_string()),
        "serialMonitor" => return Ok(wokwi::SERIAL_MONITOR.to_string()),
        "chip" => {
            return custom_chip
                .map(str::to_string)
                .ok_or_else(|| OverlayError::missing_chip(file).into());
        }
        _ => {}
    }
    // `{{ <id>.<attr> }}`: find a pin device by either spelling of its id,
    // then ask it for the attribute. No per-element `match` here.
    let Some((name, attr)) = token.rsplit_once('.') else {
        return Err(OverlayError::unknown(file, token).into());
    };
    let (name, attr) = (name.trim(), attr.trim());
    let Some(device) = board
        .devices
        .iter()
        .filter_map(espforge_elements::Device::as_pin)
        .find(|device| device.id.raw == name || device.id.rust_name() == name)
    else {
        return Err(OverlayError::unknown(file, token).into());
    };
    device
        .overlay_attr(attr)
        .ok_or_else(|| OverlayError::unknown(file, token).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_elements::{
        Console, Device, Ident, Meta, Owner, OwnerKind, Pin, PinDevice, PinUse, Runtime,
    };

    fn board() -> Board {
        Board {
            meta: Meta {
                name: "hello world".into(),
                description: None,
                chip: "esp32c3".into(),
                runtime: Runtime::Blocking,
                console: Console::Uart,
                ota: None,
            },
            resources: espforge_elements::ResourceGraph::default(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn the_two_built_in_tokens_resolve() {
        assert_eq!(
            render("{{ board }}", "diagram.json", &board(), None).unwrap(),
            "board-esp32-c3-devkitm-1"
        );
        assert_eq!(
            render("{{ serialMonitor }}", "diagram.json", &board(), None).unwrap(),
            "$serialMonitor"
        );
    }

    #[test]
    fn the_chip_token_is_the_name_the_example_declares() {
        assert_eq!(
            render(
                "chip-{{ chip }}",
                "diagram.json",
                &board(),
                Some("i2cscanner")
            )
            .unwrap(),
            "chip-i2cscanner"
        );
    }

    #[test]
    fn the_chip_token_without_a_chip_json_is_an_error() {
        let err = render("chip-{{ chip }}", "diagram.json", &board(), None).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert!(overlay.message.contains("`chip.json`"), "{overlay}");
    }

    #[test]
    fn a_whole_diagram_keeps_everything_else_verbatim() {
        let text = "{\n  \"type\": \"{{ board }}\",\n  \"c\": [\"esp:TX\", \"{{ serialMonitor }}:RX\"]\n}\n";
        let out = render(text, "diagram.json", &board(), None).unwrap();
        assert!(
            out.contains("\"type\": \"board-esp32-c3-devkitm-1\""),
            "{out}"
        );
        assert!(out.contains("\"$serialMonitor:RX\""), "{out}");
        assert!(!out.contains("{{"), "{out}");
    }

    #[test]
    fn an_unknown_token_is_an_error_that_lists_the_supported_set() {
        let err = render("{{ opt.wifi }}", "diagram.json", &board(), None).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert_eq!(overlay.file, "diagram.json");
        assert!(overlay.message.contains("unknown placeholder"), "{overlay}");
        assert!(overlay.message.contains("{{ board }}"), "{overlay}");
        assert!(overlay.message.contains("{{ <device>.pin }}"), "{overlay}");
    }

    /// `blink`: one LED whose id is written `red-led`.
    fn blink_board() -> Board {
        let mut board = board();
        board.meta.name = "blink".into();
        let id = Ident::new("red-led", 0..0);
        let pin = board
            .resources
            .claim(
                Pin(18),
                Owner::new(OwnerKind::Pin("led".into()), id.clone()),
                0..0,
                PinUse::Output,
            )
            .expect("pin 18 is free");
        board
            .devices
            .push(Device::Pin(PinDevice::new("led", id, pin)));
        board
    }

    #[test]
    fn a_device_pin_resolves_under_both_id_spellings() {
        let board = blink_board();
        assert_eq!(
            render("{{ red-led.pin }}", "d.json", &board, None).unwrap(),
            "18"
        );
        assert_eq!(
            render("{{ red_led.pin }}", "d.json", &board, None).unwrap(),
            "18"
        );
    }

    #[test]
    fn an_unsupported_device_attribute_is_an_unknown_token() {
        let err = render("{{ red_led.color }}", "diagram.json", &blink_board(), None).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert!(overlay.message.contains("unknown placeholder"), "{overlay}");
        assert!(overlay.message.contains("{{ <device>.pin }}"), "{overlay}");
    }

    #[test]
    fn a_device_that_is_not_on_the_board_is_an_unknown_token() {
        let err = render("{{ red_led.pin }}", "d.json", &board(), None).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert!(overlay.message.contains("unknown placeholder"), "{overlay}");
    }

    #[test]
    fn an_unclosed_placeholder_is_an_error() {
        let err = render("{{ board", "d.json", &board(), None).unwrap_err();
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
        assert_eq!(render(text, "d.json", &board(), None).unwrap(), text);
    }
}

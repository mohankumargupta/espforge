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
//! | `{{ <component>.pin }}` | a component's GPIO number |
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
    // `{{ <id>.<attr> }}`: find the component by either spelling of its id,
    // then ask it for the attribute. No per-element `match` here.
    let Some((name, attr)) = token.rsplit_once('.') else {
        return Err(OverlayError::unknown(file, token).into());
    };
    let (name, attr) = (name.trim(), attr.trim());
    let Some(component) = board
        .components
        .iter()
        .find(|c| c.id().raw == name || c.id().rust_name() == name)
    else {
        return Err(OverlayError::unknown(file, token).into());
    };
    component
        .overlay_attr(attr)
        .ok_or_else(|| OverlayError::unknown(file, token).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_elements::components::recipe::RecipeComponent;
    use espforge_elements::{
        Component, Console, Ident, Meta, Owner, OwnerKind, Pin, PinUse, Runtime,
    };

    fn board() -> Board {
        Board {
            meta: Meta {
                name: "hello world".into(),
                description: None,
                chip: "esp32c3".into(),
                runtime: Runtime::Blocking,
                console: Console::Uart,
            },
            resources: espforge_elements::ResourceGraph::default(),
            components: Vec::new(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn the_two_built_in_tokens_resolve() {
        assert_eq!(
            render("{{ board }}", "diagram.json", &board()).unwrap(),
            "board-esp32-c3-devkitm-1"
        );
        assert_eq!(
            render("{{ serialMonitor }}", "diagram.json", &board()).unwrap(),
            "$serialMonitor"
        );
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

    /// `blink`: one LED whose id is written `red-led`.
    fn blink_board() -> Board {
        let mut board = board();
        board.meta.name = "blink".into();
        let id = Ident::new("red-led", 0..0);
        let pin = board
            .resources
            .claim(
                Pin(18),
                Owner::new(OwnerKind::Recipe("led"), id.clone()),
                0..0,
                PinUse::Output,
            )
            .expect("pin 18 is free");
        board
            .components
            .push(Component::Recipe(RecipeComponent::new(
                "led",
                id,
                pin,
                vec![false],
            )));
        board
    }

    #[test]
    fn a_component_pin_resolves_under_both_id_spellings() {
        let board = blink_board();
        assert_eq!(render("{{ red-led.pin }}", "d.json", &board).unwrap(), "18");
        assert_eq!(render("{{ red_led.pin }}", "d.json", &board).unwrap(), "18");
    }

    #[test]
    fn an_unsupported_component_attribute_is_an_unknown_token() {
        let err = render("{{ red_led.color }}", "diagram.json", &blink_board()).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert!(overlay.message.contains("unknown placeholder"), "{overlay}");
        assert!(
            overlay.message.contains("{{ <component>.pin }}"),
            "{overlay}"
        );
    }

    #[test]
    fn a_component_that_is_not_on_the_board_is_an_unknown_token() {
        let err = render("{{ red_led.pin }}", "d.json", &board()).unwrap_err();
        let overlay = err.downcast_ref::<OverlayError>().expect("typed error");
        assert!(overlay.message.contains("unknown placeholder"), "{overlay}");
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

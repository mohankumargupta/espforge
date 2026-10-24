//! Typed wrapper for the `<led>` component.
//!
//! `<led>` is a component over `<gpio>` the same way `i2c_component` is over
//! `<i2c>`: `gpio="$<gpio id>"` must resolve to an active GPIO peripheral,
//! which must be an output pin. Presence and the `active_low` kind come from
//! `espf.schema.kdl`, generated into
//! [`schema::Attrs`](crate::components::generated_elements::led).
//!
//! This module owns the reference lookup, the slug check and the wording of
//! the errors — which match `espforge-hardware::components::led::attach_led`
//! so the hardware pilot can consume the wrapper without changing messages.

use espf::context::{ActivePeripheral, Handle, ParsedNode, RuntimeContext};

use crate::components::attrs::Issue;
use crate::components::generated_elements::led as schema;

/// A fully validated `<led>` node.
#[derive(Debug, Clone)]
pub struct LedComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `gpio` attribute.
    pub gpio_ref: &'a str,
    /// Parsed `active_low` attribute (defaults to false).
    pub active_low: bool,
    /// Handle of the referenced parent gpio peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Gpio`).
    pub peripheral: ActivePeripheral,
}

impl<'a> LedComponent<'a> {
    /// Validates and resolves a led node against the runtime context.
    pub fn resolve(node: &'a ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let attrs = schema::Attrs::from_node(node).map_err(|issue| message(node.id, issue))?;
        let id = attrs.id.value;
        let name = attrs.gpio.name;

        let handle = ctx.interner.get(name).ok_or_else(|| {
            format!(
                "<led id=\"{id}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`"
            )
        })?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| {
                format!(
                    "<led id=\"{id}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`"
                )
            })?;

        if !matches!(peripheral, ActivePeripheral::Gpio) {
            return Err(format!(
                "<led id=\"{id}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`"
            ));
        }

        let slug = id.replace('-', "_");
        if !is_rust_ident(&slug) {
            return Err(format!(
                "<led id=\"{id}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)"
            ));
        }

        Ok(Self {
            id,
            gpio_ref: attrs.gpio.raw,
            active_low: attrs.active_low.value,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

/// This element's wording for a schema-level problem.
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr: "gpio" } => {
            format!("<led id=\"{id}\"> needs gpio=\"$<gpio id>\"")
        }
        Issue::Missing { attr } => format!("Missing required attribute '{attr}'"),
        Issue::Invalid { attr, value } => {
            format!("<led id=\"{id}\"> {attr}=\"{value}\" must be \"true\" or \"false\"")
        }
    }
}

fn is_rust_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

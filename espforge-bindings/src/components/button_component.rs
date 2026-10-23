//! Typed wrapper for the `<button>` component.
//!
//! `<button>` is a component over `<gpio>` the same way `i2c_component` is
//! over `<i2c>`: `gpio="$<gpio id>"` must resolve to an active GPIO
//! peripheral, used as an input pin. `pull_up` defaults to `false`.
//!
//! Required attributes: `id`, `gpio`. Optional: `pull_up` (`true`/`false`).

use espf::context::{ActivePeripheral, Handle, RuntimeContext};

/// The attributes `resolve` requires besides `id`. Drift-checked against
/// the `espforge espf` catalog.
pub const REQUIRED_ATTRS: &[&str] = &["gpio"];

/// A fully validated `<button>` node.
#[derive(Debug, Clone)]
pub struct ButtonComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `gpio` attribute.
    pub gpio_ref: &'a str,
    /// Parsed `pull_up` attribute (defaults to false).
    pub pull_up: bool,
    /// Handle of the referenced parent gpio peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Gpio`).
    pub peripheral: ActivePeripheral,
}

impl<'a> ButtonComponent<'a> {
    /// Validates and resolves a button node against the runtime context.
    /// Error text matches `espforge-hardware::components::button::attach_button` so the
    /// hardware pilot can swap to this wrapper without changing messages.
    pub fn resolve(node: &'a espf::context::ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let id = node.id;
        if id.is_empty() {
            return Err("Missing required attribute 'id'".to_string());
        }

        let gpio_ref = node
            .get_attr("gpio")
            .ok_or_else(|| format!("<button id=\"{id}\"> needs gpio=\"$<gpio id>\""))?;

        let pull_up = match node.get_attr("pull_up") {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(format!(
                    "<button id=\"{id}\"> pull_up=\"{other}\" must be \"true\" or \"false\""
                ));
            }
        };

        let name = gpio_ref.trim_start_matches('$');
        let handle = ctx.interner.get(name).ok_or_else(|| {
            format!(
                "<button id=\"{id}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`"
            )
        })?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| {
                format!(
                    "<button id=\"{id}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`"
                )
            })?;

        if !matches!(peripheral, ActivePeripheral::Gpio) {
            return Err(format!(
                "<button id=\"{id}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`"
            ));
        }

        let slug = id.replace('-', "_");
        if !is_rust_ident(&slug) {
            return Err(format!(
                "<button id=\"{id}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)"
            ));
        }

        Ok(Self {
            id,
            gpio_ref,
            pull_up,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

fn is_rust_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

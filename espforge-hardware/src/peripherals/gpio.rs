//! `<gpio>` peripheral table (step 1d).
//!
//! `<led>` / `<button>` lowering lives in `components::{led,button}`; both
//! share [`gpio_for_handle`] below to map a resolved component handle back
//! onto this table.
//!
//! Attributes come from `espf.schema.kdl` through the generated `Attrs`; this
//! module keeps the table shape and the wording of the errors.

use espf::context::{Handle, ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_bindings::components::attrs::Issue;
use espforge_bindings::components::generated_elements::gpio as schema;

/// One `<gpio>` peripheral: id -> pin + optional direction.
#[derive(Debug, Clone)]
pub struct GpioEntry {
    pub id: String,
    pub pin: u8,
    pub direction: Option<String>,
}

/// The `<gpio>` branch of the `# peripherals` loop.
pub fn lower_gpio(n: &ParsedNode) -> Result<GpioEntry, EspfError> {
    let attrs = schema::Attrs::from_node(n).map_err(|issue| {
        let (title, message) = describe(n.id, issue);
        EspfError::at_node(n, title, message)
    })?;
    Ok(GpioEntry {
        id: n.id.to_string(),
        pin: attrs.pin.value,
        // `direction` is compared as text by `<led>`/`<button>` (an output pin
        // for one, an input pin for the other), so keep the spelling.
        direction: attrs
            .direction
            .map(|direction| direction.value.as_str().to_string()),
    })
}

/// This element's wording for a schema-level problem: a bad pin reads as a
/// pin problem, a bad `direction` as a direction problem.
fn describe(id: &str, issue: Issue<'_>) -> (&'static str, String) {
    match issue {
        Issue::Invalid {
            attr: "direction",
            value,
        } => (
            "invalid gpio direction",
            format!("<gpio id=\"{id}\"> direction=\"{value}\" must be \"output\" or \"input\""),
        ),
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => (
            "invalid gpio pin",
            format!("<gpio id=\"{id}\"> needs a numeric {attr}=\"...\""),
        ),
    }
}

/// Locate the `<gpio>` entry for a resolved component handle. The wrapper
/// already proved the handle is an active GPIO; this maps it back onto the
/// `gpios` table built from `# peripherals` (same order, same ids).
pub(crate) fn gpio_for_handle<'a>(
    gpios: &'a [GpioEntry],
    ctx: &RuntimeContext,
    handle: Handle,
    owner: &ParsedNode,
    ref_raw: &str,
) -> Result<&'a GpioEntry, EspfError> {
    gpios
        .iter()
        .find(|g| ctx.interner.get(&g.id) == Some(handle))
        .ok_or_else(|| {
            let target = ref_raw.trim_start_matches('$');
            EspfError::at_node(
                owner,
                "unknown gpio",
                format!(
                    "<{} id=\"{}\"> refers to `{target}`, which is not a <gpio> in `# peripherals`",
                    owner.tag_name, owner.id
                ),
            )
        })
}

//! `<gpio>` peripheral table (step 1d).
//!
//! `<led>` / `<button>` lowering lives in `components::{led,button}`; both
//! share [`gpio_for_handle`] below to map a resolved component handle back
//! onto this table.

use espf::context::{Handle, ParsedNode, RuntimeContext};
use espf::diag::EspfError;

/// One `<gpio>` peripheral: id -> pin + optional direction.
#[derive(Debug, Clone)]
pub struct GpioEntry {
    pub id: String,
    pub pin: u8,
    pub direction: Option<String>,
}

/// The `<gpio>` branch of the `# peripherals` loop.
pub fn lower_gpio(n: &ParsedNode) -> Result<GpioEntry, EspfError> {
    let pin = n
        .get_attr("pin")
        .and_then(|p| p.parse::<u8>().ok())
        .ok_or_else(|| {
            EspfError::at_node(
                n,
                "invalid gpio pin",
                format!("<gpio id=\"{}\"> needs a numeric pin=\"...\"", n.id),
            )
        })?;
    Ok(GpioEntry {
        id: n.id.to_string(),
        pin,
        direction: n.get_attr("direction").map(str::to_string),
    })
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

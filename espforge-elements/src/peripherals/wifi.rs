//! `<wifi>`: the on-chip Wi-Fi radio.
//!
//! It claims the `WIFI` peripheral name, so a second `<wifi>` is the ordinary
//! peripheral clash (E0704) and a chip without a radio is a located error from
//! this element (E0514) rather than a `compatible` list in the schema.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;

use crate::attrs::Issue;
use crate::chips;
use crate::element::{Element, LowerCx, Reported, Section};
use crate::generated::wifi::Attrs;
use crate::model::{Owner, OwnerKind};
use crate::registry::ELEMENTS;

/// The `<wifi>` element.
pub struct Wifi;

#[distributed_slice(ELEMENTS)]
static WIFI: &dyn Element = &Wifi;

impl Element for Wifi {
    fn tag(&self) -> &str {
        "wifi"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'_>) -> Result<(), Reported> {
        let _ = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.checked_ident(node, Section::Peripherals)?;
        let span = node_span(node);

        if !chips::has_wifi(cx.chip()) {
            return Err(cx.report(
                codes::WIFI_NO_RADIO,
                span,
                "no wifi radio",
                format!("<wifi id=\"{}\">: {} has no Wi-Fi radio", id.raw, cx.chip()),
            ));
        }

        let owner = Owner::new(OwnerKind::Wifi, id.clone());
        cx.claim_peripheral("WIFI", owner, span.clone())?;

        cx.add_radio(id.raw);
        Ok(())
    }
}

/// This element's wording for a schema-level problem: only an id can be wrong.
fn describe(cx: &mut LowerCx<'_>, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => cx.report(
            codes::WIFI_INVALID_ID,
            node_span(node),
            "invalid wifi",
            format!("<wifi id=\"{id}\"> needs an {attr}=\"...\""),
        ),
    }
}

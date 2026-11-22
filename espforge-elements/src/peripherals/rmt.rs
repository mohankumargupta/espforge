//! `<rmt>`: the chip's RMT (remote control transceiver) block.
//!
//! The element deliberately builds nothing on its own: `esp-hal`'s `Rmt` is a
//! bucket of unconfigured channel creators, not a usable device. It validates
//! the chip, fixes the source clock, and publishes the bus that `<rmt_tx>` /
//! `<rmt_rx>` channels (or a `bus "rmt"` driver) allocate a channel from.
//! A project with only `<rmt>` therefore emits no `Devices` field.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;

use crate::attrs::Issue;
use crate::element::{Element, LowerCx, Reported, Section};
use crate::generated::rmt::Attrs;
use crate::model::{Bus, Owner, OwnerKind};
use crate::peripherals::parse_khz;
use crate::registry::ELEMENTS;

/// The `<rmt>` element.
pub struct Rmt;

#[distributed_slice(ELEMENTS)]
static RMT: &dyn Element = &Rmt;

impl Element for Rmt {
    fn tag(&self) -> &str {
        "rmt"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'_>) -> Result<(), Reported> {
        // The chip gate comes first: on a part without RMT, no id or
        // frequency can make the element buildable.
        let caps = rmt_caps_or_report(cx, node)?;

        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.checked_ident(node, Section::Peripherals)?;
        if id.raw != "rmt0" {
            return Err(cx.report(
                codes::RMT_INVALID_ID,
                node_span(node),
                "invalid rmt id",
                format!(
                    "<rmt id=\"{}\">: the chip has one RMT block, so use rmt0",
                    id.raw
                ),
            ));
        }

        let khz = match attrs.frequency {
            None => caps.base_mhz * 1_000,
            Some(frequency) => {
                let khz = parse_khz(frequency.value, "rmt", &id.raw).map_err(|label| {
                    cx.report(
                        codes::RMT_INVALID_FREQUENCY,
                        node_span(node),
                        "invalid rmt frequency",
                        label,
                    )
                })?;
                if khz == 0 {
                    return Err(cx.report(
                        codes::RMT_INVALID_FREQUENCY,
                        node_span(node),
                        "invalid rmt frequency",
                        format!(
                            "<rmt id=\"{}\">: frequency must be greater than zero",
                            id.raw
                        ),
                    ));
                }
                khz
            }
        };

        let span = node_span(node);
        let owner = Owner::new(OwnerKind::Rmt, id.clone());
        // One `Rmt::new` per build: a second `<rmt>` would need a second handle
        // to the same silicon.
        cx.claim_peripheral("RMT", owner, span)?;

        cx.add_bus(Bus::Rmt {
            id,
            peripheral: "RMT".to_string(),
            khz,
        });
        Ok(())
    }
}

/// The chip's RMT table, or a located `E0530` when it has none.
fn rmt_caps_or_report(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
) -> Result<crate::chips::RmtCaps, Reported> {
    let chip = cx.chip();
    match crate::chips::rmt_caps(chip) {
        Some(caps) => Ok(caps),
        None => {
            let why = if crate::chips::has_rmt(chip) {
                format!("espforge has no verified RMT channel table for {chip} yet")
            } else {
                format!("{chip} has no RMT peripheral")
            };
            Err(cx.report(
                codes::RMT_NOT_SUPPORTED,
                node_span(node),
                "RMT is not available on this chip",
                format!("<rmt id=\"{}\">: {why}", node.id),
            ))
        }
    }
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx<'_>, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id.as_ref();
    match issue {
        Issue::Unknown { attr, known } => cx.report(
            codes::ATTR_UNKNOWN,
            crate::attrs::key_span_of(node, attr),
            "unknown attribute",
            format!(
                "<rmt id=\"{id}\"> {}",
                crate::attrs::unknown_attr(attr, known)
            ),
        ),
        Issue::Missing { attr } | Issue::Invalid { attr, .. } if attr == "id" => cx.report(
            codes::RMT_INVALID_ID,
            node_span(node),
            "invalid rmt id",
            "<rmt> needs id=\"rmt0\"".to_string(),
        ),
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => cx.report(
            codes::RMT_INVALID_FREQUENCY,
            node_span(node),
            "invalid rmt frequency",
            format!("<rmt id=\"{id}\"> needs a frequency like {attr}=\"...\""),
        ),
    }
}

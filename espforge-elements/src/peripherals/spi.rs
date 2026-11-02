//! `<spi>`: an SPI controller, its pins and its mode.
//!
//! The id must be `spi2`: that is the peripheral name the runtime bus is built
//! from, and relaxing it to `spiN` would guess which chips expose `SPI3`.
//! `frequency` has no schema default: an omitted one is the runtime's 1MHz.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;

use crate::attrs::Issue;
use crate::element::{Element, LowerCx, Reported};
use crate::generated::spi::Attrs;
use crate::model::{Bus, Owner, OwnerKind, Pin, PinUse};
use crate::peripherals::parse_khz;
use crate::registry::ELEMENTS;

/// The `<spi>` element.
pub struct Spi;

#[distributed_slice(ELEMENTS)]
static SPI: &dyn Element = &Spi;

impl Element for Spi {
    fn tag(&self) -> &'static str {
        "spi"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let id = cx.ident(node);
        if id.raw != "spi2" {
            return Err(cx.report(
                codes::SPI_INVALID_ID,
                node_span(node),
                "invalid spi id",
                format!("<spi id=\"{}\">: the id must be spi2", id.raw),
            ));
        }

        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;

        // The runtime's `SpiConfig` default is 1MHz, so an omitted frequency
        // is not "0kHz".
        let khz = match attrs.frequency {
            None => 1000,
            Some(frequency) => parse_khz(frequency.value, "spi", &id.raw).map_err(|label| {
                cx.report(
                    codes::SPI_INVALID_FREQUENCY,
                    node_span(node),
                    "invalid spi frequency",
                    label,
                )
            })?,
        };

        let span = node_span(node);
        let owner = Owner::new(OwnerKind::Spi, id.clone());
        let sclk = claim_bus_pin(cx, attrs.sclk.value, owner.clone(), span.clone())?;
        let mosi = claim_bus_pin(cx, attrs.mosi.value, owner.clone(), span.clone())?;
        let miso = match attrs.miso {
            Some(miso) => Some(claim_bus_pin(cx, miso.value, owner, span)?),
            None => None,
        };

        cx.add_bus(Bus::Spi {
            id,
            peripheral: "SPI2".to_string(),
            sclk,
            mosi,
            miso,
            khz,
            mode: attrs.mode.value as u8,
        });
        Ok(())
    }
}

fn claim_bus_pin(
    cx: &mut LowerCx,
    pin: u8,
    owner: Owner,
    span: std::ops::Range<usize>,
) -> Result<crate::model::ClaimedPin, Reported> {
    cx.claim_pin(Pin(pin), owner, span, PinUse::Bidirectional)
        .map_err(|conflict| cx.report_conflict(&conflict))
}

/// This element's wording for a schema-level problem: `mode` keeps its own
/// title, everything else is a pin problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        Issue::Invalid {
            attr: "mode",
            value,
        } => cx.report(
            codes::SPI_INVALID_MODE,
            node_span(node),
            "invalid spi mode",
            format!("<spi id=\"{id}\"> mode=\"{value}\" must be 0, 1, 2 or 3"),
        ),
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => cx.report(
            codes::SPI_INVALID_PIN,
            node_span(node),
            "invalid spi pin",
            format!("<spi id=\"{id}\"> needs a numeric {attr}=\"...\""),
        ),
    }
}

//! `<i2c>`: an I2C controller and the two pins it owns.
//!
//! The id must be `i2c0`, `i2c1` or `i2c2`, because those are the esp-hal
//! peripheral structs; the frequency defaults to `100kHz` in the schema.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;

use crate::attrs::Issue;
use crate::element::{Element, LowerCx, Reported, Section};
use crate::generated::i2c::Attrs;
use crate::model::{Bus, Owner, OwnerKind, Pin, PinUse};
use crate::peripherals::parse_khz;
use crate::registry::ELEMENTS;

/// The `<i2c>` element.
pub struct I2c;

#[distributed_slice(ELEMENTS)]
static I2C: &dyn Element = &I2c;

impl Element for I2c {
    fn tag(&self) -> &str {
        "i2c"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'_>) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.checked_ident(node, Section::Peripherals)?;

        let peripheral = match id.raw.as_str() {
            "i2c0" | "i2c1" | "i2c2" => id.raw.to_ascii_uppercase(),
            _ => {
                return Err(cx.report(
                    codes::I2C_INVALID_ID,
                    node_span(node),
                    "invalid i2c id",
                    format!("<i2c id=\"{}\">: the id must be i2c0, i2c1 or i2c2", id.raw),
                ));
            }
        };

        let khz = parse_khz(attrs.frequency.value, "i2c", &id.raw).map_err(|label| {
            cx.report(
                codes::I2C_INVALID_FREQUENCY,
                node_span(node),
                "invalid i2c frequency",
                label,
            )
        })?;

        let span = node_span(node);
        let owner = Owner::new(OwnerKind::I2c, id.clone());
        let sda = claim_bus_pin(cx, attrs.sda.value, owner.clone(), span.clone())?;
        let scl = claim_bus_pin(cx, attrs.scl.value, owner, span)?;

        cx.add_bus(Bus::I2c {
            id,
            peripheral,
            sda,
            scl,
            khz,
        });
        Ok(())
    }
}

fn claim_bus_pin(
    cx: &mut LowerCx<'_>,
    pin: u8,
    owner: Owner,
    span: std::ops::Range<usize>,
) -> Result<crate::model::ClaimedPin, Reported> {
    cx.claim_pin(Pin(pin), owner, span, PinUse::Bidirectional)
        .map_err(|refusal| refusal.report(cx))
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx<'_>, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => cx.report(
            codes::I2C_INVALID_PIN,
            node_span(node),
            "invalid i2c pin",
            format!("<i2c id=\"{id}\"> needs a numeric {attr}=\"...\""),
        ),
    }
}

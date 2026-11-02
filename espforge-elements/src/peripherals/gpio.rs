//! `<gpio>`: a single GPIO pin.
//!
//! It records the declaration and claims nothing; the first component that
//! uses the pin claims it, as in the legacy lowering.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;

use crate::element::{Element, LowerCx, Reported};
use crate::generated::gpio::{Attrs, Direction as SchemaDirection};
use crate::model::{Direction, GpioDecl};
use crate::registry::ELEMENTS;

/// The `<gpio>` element.
pub struct Gpio;

#[distributed_slice(ELEMENTS)]
static GPIO: &dyn Element = &Gpio;

impl Element for Gpio {
    fn tag(&self) -> &'static str {
        "gpio"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        cx.add_gpio(GpioDecl {
            id: cx.ident(node),
            pin: attrs.pin.value,
            direction: attrs.direction.map(|direction| match direction.value {
                SchemaDirection::Output => Direction::Output,
                SchemaDirection::Input => Direction::Input,
            }),
            span: node_span(node),
        });
        Ok(())
    }
}

/// This element's wording for a schema-level problem: a bad pin reads as a
/// pin problem, a bad `direction` as a direction problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: crate::attrs::Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        crate::attrs::Issue::Invalid {
            attr: "direction",
            value,
        } => cx.report(
            codes::GPIO_INVALID_DIRECTION,
            node_span(node),
            "invalid gpio direction",
            format!("<gpio id=\"{id}\"> direction=\"{value}\" must be \"output\" or \"input\""),
        ),
        crate::attrs::Issue::Missing { attr } | crate::attrs::Issue::Invalid { attr, .. } => cx
            .report(
                codes::GPIO_INVALID_PIN,
                node_span(node),
                "invalid gpio pin",
                format!("<gpio id=\"{id}\"> needs a numeric {attr}=\"...\""),
            ),
    }
}

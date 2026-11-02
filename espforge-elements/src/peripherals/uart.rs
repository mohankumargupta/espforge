//! `<uart>`: a UART port, its physical peripheral and its pins.
//!
//! The id names the peripheral (`uart1` -> `UART1`) unless `uart="UARTn"`
//! says it explicitly; then the id is just a reference name. Only `<uart>`
//! claims a physical peripheral name, because `spi2`/`i2cN` are already
//! encoded in their id rules.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;

use crate::attrs::Issue;
use crate::element::{Element, LowerCx, Reported};
use crate::generated::uart::Attrs;
use crate::model::{Bus, Owner, OwnerKind, Pin, PinUse};
use crate::registry::ELEMENTS;

/// The `<uart>` element.
pub struct Uart;

#[distributed_slice(ELEMENTS)]
static UART: &dyn Element = &Uart;

impl Element for Uart {
    fn tag(&self) -> &'static str {
        "uart"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.ident(node);
        let span = node_span(node);

        let peripheral = match attrs.uart {
            // Explicit physical peripheral; the id is just a reference name.
            Some(raw) => {
                let upper = raw.value.trim().to_ascii_uppercase();
                if !matches!(upper.as_str(), "UART0" | "UART1" | "UART2" | "UART3") {
                    return Err(cx.report(
                        codes::UART_INVALID_PERIPHERAL,
                        span,
                        "invalid uart peripheral",
                        format!(
                            "<uart id=\"{}\"> uart=\"{}\" must be UART0, UART1, UART2 or UART3",
                            id.raw, raw.value
                        ),
                    ));
                }
                upper
            }
            // Backwards compatible: the id itself names the peripheral.
            None => match id.raw.as_str() {
                "uart0" | "uart1" | "uart2" | "uart3" => id.raw.to_ascii_uppercase(),
                _ => {
                    return Err(cx.report(
                        codes::UART_INVALID_ID,
                        span,
                        "invalid uart id",
                        format!(
                            "<uart id=\"{}\">: the id must be uart0, uart1, uart2 or uart3 (or give uart=\"UART1\" explicitly)",
                            id.raw
                        ),
                    ));
                }
            },
        };

        let owner = Owner::new(OwnerKind::Uart, id.clone());
        cx.claim_peripheral(peripheral.clone(), owner.clone(), span.clone())
            .map_err(|clash| {
                cx.report(
                    codes::CONFLICT_PERIPHERAL,
                    clash.existing.span.clone(),
                    "peripheral clash",
                    format!(
                        "{} and {} both use {}",
                        display(&clash.existing.owner),
                        display(&clash.incoming.owner),
                        clash.peripheral
                    ),
                )
            })?;

        let tx = claim_bus_pin(cx, attrs.tx.value, owner.clone(), span.clone())?;
        let rx = claim_bus_pin(cx, attrs.rx.value, owner, span)?;

        cx.add_bus(Bus::Uart {
            id,
            peripheral,
            tx,
            rx,
            used_by: None,
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
    cx.claim_pin(Pin(pin), owner, span, PinUse::Unspecified)
        .map_err(|conflict| cx.report_conflict(&conflict))
}

fn display(owner: &Owner) -> String {
    format!("<{} id=\"{}\">", owner.kind.tag(), owner.id.raw)
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        Issue::Missing { attr } | Issue::Invalid { attr, .. } => cx.report(
            codes::UART_INVALID_PIN,
            node_span(node),
            "invalid uart pin",
            format!("<uart id=\"{id}\"> needs a numeric {attr}=\"...\""),
        ),
    }
}

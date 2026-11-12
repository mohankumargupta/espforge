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
use crate::element::{Element, LowerCx, Reported, Section};
use crate::generated::uart::{Attrs, Parity as SchemaParity, StopBits as SchemaStopBits};
use crate::model::{Bus, Framing, Owner, OwnerKind, Parity, Pin, PinUse, StopBits};
use crate::registry::ELEMENTS;

/// The `<uart>` element.
pub struct Uart;

#[distributed_slice(ELEMENTS)]
static UART: &dyn Element = &Uart;

impl Element for Uart {
    fn tag(&self) -> &str {
        "uart"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'_>) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.checked_ident(node, Section::Peripherals)?;
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

        if attrs.baud_rate.value == 0 {
            return Err(cx.report(
                codes::UART_INVALID_BAUD,
                span,
                "invalid baud_rate",
                format!(
                    "<uart id=\"{}\"> baud_rate=\"0\" must be greater than 0",
                    id.raw
                ),
            ));
        }

        let owner = Owner::new(OwnerKind::Uart, id.clone());
        cx.claim_peripheral(peripheral.clone(), owner.clone(), span.clone())?;

        let tx = claim_bus_pin(cx, attrs.tx.value, owner.clone(), span.clone())?;
        let rx = claim_bus_pin(cx, attrs.rx.value, owner, span)?;

        cx.add_bus(Bus::Uart {
            id,
            peripheral,
            tx,
            rx,
            baud: attrs.baud_rate.value,
            framing: Framing {
                parity: match attrs.parity.value {
                    SchemaParity::None => Parity::None,
                    SchemaParity::Even => Parity::Even,
                    SchemaParity::Odd => Parity::Odd,
                },
                stop_bits: match attrs.stop_bits.value {
                    SchemaStopBits::N1 => StopBits::One,
                    SchemaStopBits::N2 => StopBits::Two,
                },
            },
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
    cx.claim_pin(Pin(pin), owner, span, PinUse::Unspecified)
        .map_err(|refusal| refusal.report(cx))
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx<'_>, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
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

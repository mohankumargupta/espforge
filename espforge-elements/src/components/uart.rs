//! `<uart_component>`: the framing settings of one UART port.
//!
//! Unlike I2C, the port is built inside the initialiser: one component per
//! `<uart>`, and the generated field owns the driver directly.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;
use quote::{format_ident, quote};

use crate::attrs::Issue;
use crate::components::{gpio, parity, peripheral_access, runtime_type, stop_bits};
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, is_rust_ident};
use crate::generated::uart_component::{Attrs, Parity as SchemaParity, StopBits as SchemaStopBits};
use crate::model::{Board, Bus, BusId, Component, Framing, Ident, Parity, StopBits};
use crate::registry::ELEMENTS;

/// The `<uart_component>` element.
pub struct UartComponentElement;

#[distributed_slice(ELEMENTS)]
static UART_COMPONENT: &dyn Element = &UartComponentElement;

impl Element for UartComponentElement {
    fn tag(&self) -> &'static str {
        "uart_component"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.ident(node);
        let bus_name = attrs.uart.name;

        let Some(bus) = cx.bus_by_name(bus_name) else {
            return Err(cx.report(
                codes::COMPONENT_UNKNOWN_BUS,
                node_span(node),
                "unknown uart bus",
                format!(
                    "<uart_component id=\"{}\"> refers to `{bus_name}`, which is not an <uart> in `# peripherals`",
                    id.raw
                ),
            ));
        };

        if let Some(other) = used_by(cx, bus) {
            let bus_id = cx
                .bus(bus)
                .map_or("", |bus| bus.id().raw.as_str())
                .to_string();
            return Err(cx.report(
                codes::COMPONENT_UART_USED,
                node_span(node),
                "uart already used",
                format!(
                    "<uart_component id=\"{}\"> and <uart_component id=\"{}\"> both use <uart id=\"{bus_id}\">",
                    other.rust_name(),
                    id.raw.replace('-', "_")
                ),
            ));
        }

        if attrs.baud_rate.value == 0 {
            return Err(cx.report(
                codes::COMPONENT_INVALID_BAUD,
                node_span(node),
                "invalid baud_rate",
                format!(
                    "<uart_component id=\"{}\"> baud_rate=\"0\" must be greater than 0",
                    id.raw
                ),
            ));
        }

        if !is_rust_ident(&id.rust_name()) {
            return Err(cx.report(
                codes::COMPONENT_INVALID_ID,
                node_span(node),
                "invalid uart_component",
                format!(
                    "<uart_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                    id.raw
                ),
            ));
        }

        set_used_by(cx, bus, id.clone());
        cx.push(Component::Uart(UartComponent {
            id,
            port: bus,
            baud: attrs.baud_rate.value,
            framing: Framing {
                parity: ir_parity(attrs.parity.value),
                stop_bits: ir_stop_bits(attrs.stop_bits.value),
            },
        }));
        Ok(())
    }
}

/// One lowered `<uart_component>` on a UART port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UartComponent {
    pub id: Ident,
    pub port: BusId,
    pub baud: u32,
    pub framing: Framing,
}

impl Lowered for UartComponent {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, board: &Board, cx: &mut EmitCx) {
        let Some(Bus::Uart {
            peripheral, tx, rx, ..
        }) = board.resources.bus(self.port)
        else {
            return;
        };
        let name = format_ident!("{}", self.id.rust_name());
        let runtime = runtime_type(board.meta.runtime);
        let peripheral = peripheral_access(peripheral);
        let (tx_pin, rx_pin) = (tx.pin().0, rx.pin().0);
        let tx = gpio(tx_pin);
        let rx = gpio(rx_pin);
        let baud = crate::components::int(self.baud);
        let parity = parity(self.framing.parity);
        let stop_bits = stop_bits(self.framing.stop_bits);

        cx.field(quote! {
            pub #name: espforge_runtime::UartDevice<#runtime>,
        });
        cx.init(quote! {
            #name: espforge_runtime::UartDevice::build(
                #peripheral,
                #tx,
                #rx,
                espforge_runtime::UartConfig {
                    baudrate: #baud,
                    data_bits: esp_hal::uart::DataBits::_8,
                    parity: #parity,
                    stop_bits: #stop_bits,
                },
            )
            .expect("UART configuration failed"),
        });
        cx.safety_block(
            format!("{name}: espforge_runtime::UartDevice::build("),
            &[
                "SAFETY: espforge checked that no other component uses this",
                &format!("peripheral or GPIO{tx_pin}/GPIO{rx_pin}."),
            ],
        );
    }
}

/// The component riding on a bus, if any.
fn used_by(cx: &LowerCx, bus: BusId) -> Option<Ident> {
    match cx.bus(bus) {
        Some(Bus::Uart { used_by, .. }) => used_by.clone(),
        _ => None,
    }
}

/// Record the component that rides on a bus.
fn set_used_by(cx: &mut LowerCx, bus: BusId, id: Ident) {
    if let Some(Bus::Uart { used_by, .. }) = cx.bus_mut(bus) {
        *used_by = Some(id);
    }
}

/// Map the schema's `parity` choice onto the IR's own vocabulary.
fn ir_parity(parity: SchemaParity) -> Parity {
    match parity {
        SchemaParity::None => Parity::None,
        SchemaParity::Even => Parity::Even,
        SchemaParity::Odd => Parity::Odd,
    }
}

/// Map the schema's `stop_bits` choice onto the IR's own vocabulary.
fn ir_stop_bits(stop_bits: SchemaStopBits) -> StopBits {
    match stop_bits {
        SchemaStopBits::N1 => StopBits::One,
        SchemaStopBits::N2 => StopBits::Two,
    }
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    let label = match issue {
        Issue::Missing { attr } => {
            format!("❌ [uart_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid {
            attr: "baud_rate",
            value,
        } => format!("❌ [uart_component] '{id}' invalid baud_rate '{value}'"),
        Issue::Invalid {
            attr: "parity",
            value,
        } => format!(
            "❌ [uart_component] Invalid parity '{value}': expected \"none\", \"even\" or \"odd\""
        ),
        Issue::Invalid { value, .. } => {
            format!("❌ [uart_component] Invalid stop_bits '{value}': expected \"1\" or \"2\"")
        }
    };
    cx.report(
        codes::COMPONENT_INVALID_UART,
        node_span(node),
        "invalid uart_component",
        label,
    )
}

//! `<i2c_component>`: one endpoint on an `<i2c>` bus.
//!
//! I2C is a multi-drop bus: several components may ride on one `<i2c>`, each
//! at its own 7-bit address. `address` is what makes a component an *addressed
//! endpoint*; a component without one is the raw bus itself (an I2C scanner
//! sweeps every address, so it has none).
//!
//! Emission follows SPI: the controller is built once in a prelude `let`
//! ([`EmitCx::once`]), and each component either wraps that handle in an
//! `I2cDevice` carrying its address or takes the handle unchanged. Two
//! addressed components on one bus may not share an address — the wiring bug
//! only lowering can catch.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;
use proc_macro2::Literal;
use quote::{format_ident, quote};

use crate::attrs::Issue;
use crate::components::{gpio, peripheral_access, rate, runtime_type};
use crate::devices::span_of;
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, is_rust_ident};
use crate::generated::i2c_component::Attrs;
use crate::model::{Board, Bus, BusId, Component, Ident};
use crate::registry::ELEMENTS;

/// The `<i2c_component>` element.
pub struct I2cComponentElement;

#[distributed_slice(ELEMENTS)]
static I2C_COMPONENT: &dyn Element = &I2cComponentElement;

impl Element for I2cComponentElement {
    fn tag(&self) -> &'static str {
        "i2c_component"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.ident(node);
        let address = attrs.address.map(|address| address.value);
        let bus_name = attrs.i2c.name;

        let Some(bus) = cx.bus_by_name(bus_name) else {
            return Err(cx.report(
                codes::COMPONENT_UNKNOWN_BUS,
                node_span(node),
                "unknown i2c bus",
                format!(
                    "<i2c_component id=\"{}\"> refers to `{bus_name}`, which is not an <i2c> in `# peripherals`",
                    id.raw
                ),
            ));
        };

        if !is_rust_ident(&id.rust_name()) {
            return Err(invalid_id(cx, node, "i2c_component", &id));
        }

        if let Some(address) = address {
            // The specification reserves 0x00..=0x07 (general call, CBUS) and
            // 0x78..=0x7F (10-bit prefixes, reserved): neither range can be a
            // usable 7-bit slave address.
            if !(0x08..=0x77).contains(&address) {
                return Err(cx.report(
                    codes::COMPONENT_INVALID_I2C,
                    span_of(node, "address"),
                    "invalid i2c_component",
                    format!(
                        "<i2c_component id=\"{}\"> address=\"0x{address:02X}\" is reserved by the \
                         I2C specification; usable addresses are 0x08..=0x77",
                        id.raw
                    ),
                ));
            }

            // Two parts answering to one address on one controller cannot be
            // addressed independently. Collect the clash first: `report` needs
            // `&mut cx`, which the component list borrow forbids.
            let clash = cx
                .components()
                .iter()
                .find_map(|component| match component {
                    Component::I2c(existing)
                        if existing.bus == bus && existing.address == Some(address) =>
                    {
                        Some(existing.id.clone())
                    }
                    _ => None,
                });
            if let Some(other) = clash {
                let bus_id = cx
                    .bus(bus)
                    .map_or_else(String::new, |bus| bus.id().raw.clone());
                return Err(cx.report(
                    codes::CONFLICT_I2C_ADDRESS,
                    node_span(node),
                    "i2c address conflict",
                    format!(
                        "<i2c_component id=\"{}\"> and <i2c_component id=\"{}\"> both use address \
                         0x{address:02X} on <i2c id=\"{bus_id}\">",
                        other.raw, id.raw
                    ),
                ));
            }
        }

        cx.push(Component::I2c(I2cComponent { id, bus, address }));
        Ok(())
    }
}

/// One lowered `<i2c_component>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cComponent {
    pub id: Ident,
    pub bus: BusId,
    /// The address the component declares, when it has one. A bus-level
    /// component such as an I2C scanner deliberately has none.
    pub address: Option<u8>,
}

impl Lowered for I2cComponent {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, board: &Board, cx: &mut EmitCx) {
        let Some(Bus::I2c {
            id: bus_id,
            peripheral,
            sda,
            scl,
            khz,
        }) = board.resources.bus(self.bus)
        else {
            return;
        };

        let handle = format_ident!("{}_bus", bus_id.rust_name());
        let runtime = runtime_type(board.meta.runtime);
        let peripheral_tokens = peripheral_access(peripheral);
        let (sda_pin, scl_pin) = (sda.pin().0, scl.pin().0);
        let sda_tokens = gpio(sda_pin);
        let scl_tokens = gpio(scl_pin);
        let rate_tokens = rate(*khz);

        // One controller per bus: the first component builds it, the rest ride
        // on the same handle, and a bus with no component is never built.
        let key = handle.to_string();
        let inserted = cx.once(
            &key,
            quote! {
                let #handle = espforge_runtime::I2cBus::build(
                    #peripheral_tokens,
                    #sda_tokens,
                    #scl_tokens,
                    espforge_runtime::I2cConfig { frequency: #rate_tokens },
                )
                .expect("I2C bus configuration failed");
            },
        );
        if inserted {
            cx.safety_block(
                format!("let {handle} ="),
                &[
                    "SAFETY: espforge checked that no other component uses this",
                    &format!("peripheral or GPIO{sda_pin}/GPIO{scl_pin}."),
                ],
            );
        }

        let name = format_ident!("{}", self.id.rust_name());
        match self.address {
            Some(address) => {
                // Unsuffixed, like the integer components: the golden prints
                // `0x3C` as `60`.
                let address = Literal::u8_unsuffixed(address);
                cx.field(quote! {
                    pub #name: espforge_runtime::I2cDevice<#runtime>,
                });
                cx.init(quote! {
                    #name: espforge_runtime::I2cDevice::new(#handle, #address),
                });
            }
            None => {
                // A bus-level component (a scanner) keeps the raw handle: it
                // has to be able to address every slave in turn.
                cx.field(quote! {
                    pub #name: espforge_runtime::I2cBus<#runtime>,
                });
                cx.init(quote! {
                    #name: #handle,
                });
            }
        }
    }
}

fn invalid_id(cx: &mut LowerCx, node: &ParsedNode<'_>, tag: &str, id: &Ident) -> Reported {
    cx.report(
        codes::COMPONENT_INVALID_ID,
        node_span(node),
        format!("invalid {tag}"),
        format!(
            "<{tag} id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
            id.raw
        ),
    )
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        Issue::Missing { attr } => cx.report(
            codes::COMPONENT_INVALID_I2C,
            node_span(node),
            "invalid i2c_component",
            format!("<i2c_component id=\"{id}\"> missing required attribute '{attr}'"),
        ),
        Issue::Invalid { value, .. } if value.starts_with("0x") || value.starts_with("0X") => cx
            .report(
                codes::COMPONENT_INVALID_I2C,
                node_span(node),
                "invalid i2c_component",
                format!("❌ [i2c_component] '{id}' invalid hex address '{value}'"),
            ),
        Issue::Invalid { value, .. } => cx.report(
            codes::COMPONENT_INVALID_I2C,
            node_span(node),
            "invalid i2c_component",
            format!("❌ [i2c_component] '{id}' invalid decimal address '{value}'"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Console, Meta, Owner, OwnerKind, Pin, PinUse, ResourceGraph, Runtime};

    fn board() -> Board {
        Board {
            meta: Meta {
                name: "i2c".into(),
                description: None,
                chip: "esp32c3".into(),
                runtime: Runtime::Blocking,
                console: Console::Uart,
            },
            resources: ResourceGraph::default(),
            components: Vec::new(),
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// One `i2c0` bus with `devices` endpoints on it.
    fn bus_with(devices: &[(&str, Option<u8>)]) -> (Board, Vec<I2cComponent>) {
        let mut board = board();
        let owner = Owner::new(OwnerKind::I2c, Ident::new("i2c0", 0..0));
        let claim = |board: &mut Board, pin: u8| {
            board
                .resources
                .claim(Pin(pin), owner.clone(), 0..0, PinUse::Bidirectional)
                .expect("the test pins are free")
        };
        let sda = claim(&mut board, 6);
        let scl = claim(&mut board, 7);
        let bus = board.resources.add_bus(Bus::I2c {
            id: Ident::new("i2c0", 0..0),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
        });
        let components = devices
            .iter()
            .map(|(id, address)| I2cComponent {
                id: Ident::new(*id, 0..0),
                bus,
                address: *address,
            })
            .collect();
        (board, components)
    }

    #[test]
    fn two_endpoints_on_one_bus_emit_one_bus_let() {
        let (board, components) = bus_with(&[("display", Some(0x3C)), ("sensor", Some(0x62))]);
        let mut cx = EmitCx::new();
        for component in &components {
            component.emit(&board, &mut cx);
        }
        let parts = cx.into_parts();
        assert_eq!(parts.prelude.len(), 1, "the controller is built once");
        assert_eq!(parts.fields.len(), 2);
        assert_eq!(parts.inits.len(), 2);
        // The safety note is emitted once, with the prelude.
        assert_eq!(
            parts
                .safety
                .iter()
                .filter(|(anchor, _)| anchor == "let i2c0_bus =")
                .count(),
            1
        );
        let fields = parts
            .fields
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(fields.contains("I2cDevice"), "{fields}");
        let inits = parts
            .inits
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        // Unsuffixed, so the golden reads `0x3C` as `60` and `0x62` as `98`.
        assert!(
            inits.contains("I2cDevice :: new (i2c0_bus , 60)"),
            "{inits}"
        );
        assert!(
            inits.contains("I2cDevice :: new (i2c0_bus , 98)"),
            "{inits}"
        );
    }

    #[test]
    fn an_unaddressed_endpoint_emits_the_raw_bus() {
        let (board, components) = bus_with(&[("scanner", None)]);
        let mut cx = EmitCx::new();
        components[0].emit(&board, &mut cx);
        let parts = cx.into_parts();
        assert_eq!(
            parts.fields[0].to_string(),
            "pub scanner : espforge_runtime :: I2cBus < esp_hal :: Blocking > ,"
        );
        assert_eq!(parts.inits[0].to_string(), "scanner : i2c0_bus ,");
    }
}

//! `<spi_component>`: one chip select on an `<spi>` bus.
//!
//! Unlike I2C and UART, the bus is built before `Self { .. }`: a device takes
//! the bus handle by value, so every device on the bus shares one `let`. That
//! is what [`EmitCx::once`] is for: the first device builds the bus, later
//! ones reuse it, and a bus with no device is never built.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;
use quote::{format_ident, quote};

use crate::attrs::{Issue, PinOrRef};
use crate::components::{gpio, peripheral_access, rate, runtime_type, spi_mode};
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, is_rust_ident};
use crate::generated::spi_component::Attrs;
use crate::model::{Board, Bus, BusId, Component, Ident, Owner, OwnerKind, Pin, PinUse};
use crate::registry::ELEMENTS;

/// The `<spi_component>` element.
pub struct SpiComponentElement;

#[distributed_slice(ELEMENTS)]
static SPI_COMPONENT: &dyn Element = &SpiComponentElement;

impl Element for SpiComponentElement {
    fn tag(&self) -> &'static str {
        "spi_component"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.ident(node);

        // A chip select written as a `<gpio>` id must name a declared gpio.
        let cs_gpio = match attrs.cs {
            PinOrRef::Pin(pin) => {
                let pin = pin.value;
                (pin, None)
            }
            PinOrRef::Ref(reference) => {
                let Some(gpio) = cx.gpio(reference.name).cloned() else {
                    return Err(cx.report(
                        codes::COMPONENT_INVALID_CS,
                        node_span(node),
                        "invalid spi_component",
                        format!(
                            "❌ [spi_component] '{}' cs references undefined gpio '{}'",
                            id.raw, reference.name
                        ),
                    ));
                };
                (gpio.pin, Some(gpio.id.raw))
            }
        };
        let (cs_pin, cs_gpio) = cs_gpio;

        let bus_name = attrs.spi.name;
        let Some(bus) = cx.bus_by_name(bus_name) else {
            return Err(cx.report(
                codes::COMPONENT_UNKNOWN_BUS,
                node_span(node),
                "unknown spi bus",
                format!(
                    "<spi_component id=\"{}\"> refers to `{bus_name}`, which is not an <spi> in `# peripherals`",
                    id.raw
                ),
            ));
        };

        if !is_rust_ident(&id.rust_name()) {
            return Err(cx.report(
                codes::COMPONENT_INVALID_ID,
                node_span(node),
                "invalid spi_component",
                format!(
                    "<spi_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                    id.raw
                ),
            ));
        }

        let cs = cx
            .claim_pin(
                Pin(cs_pin),
                Owner::new(OwnerKind::SpiComponent, id.clone()),
                node_span(node),
                PinUse::Unspecified,
            )
            .map_err(|conflict| cx.report_conflict(&conflict))?;
        if let Some(gpio) = cs_gpio {
            cx.record_cs_gpio(gpio);
        }

        cx.push(Component::Spi(SpiComponent {
            id,
            bus,
            cs,
            mode: attrs.mode.value as u8,
        }));
        Ok(())
    }
}

/// One lowered `<spi_component>` on a bus, with its own chip-select claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiComponent {
    pub id: Ident,
    pub bus: BusId,
    pub cs: crate::model::ClaimedPin,
    /// Read and carried, but the bus's `<spi mode>` is the one configured.
    pub mode: u8,
}

impl Lowered for SpiComponent {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, board: &Board, cx: &mut EmitCx) {
        let Some(bus) = board.resources.bus(self.bus) else {
            return;
        };
        let Bus::Spi {
            id: bus_id,
            peripheral,
            sclk,
            mosi,
            miso,
            khz,
            mode,
        } = bus
        else {
            return;
        };

        let handle = format_ident!("{}_bus", bus_id.rust_name());
        let peripheral = peripheral_access(peripheral);
        let sclk_pin = sclk.pin().0;
        let mosi_pin = mosi.pin().0;
        let miso_pin = miso.as_ref().map(|pin| pin.pin().0);
        let sclk = gpio(sclk_pin);
        let mosi = gpio(mosi_pin);
        let rate = rate(*khz);
        let bus_mode = spi_mode(*mode);

        let key = handle.to_string();
        let inserted = match miso_pin {
            Some(miso_pin) => {
                let miso = gpio(miso_pin);
                cx.once(
                    &key,
                    quote! {
                        let #handle = espforge_runtime::SpiBus::build(
                            #peripheral, #sclk, #mosi, #miso,
                            espforge_runtime::SpiConfig {
                                frequency: #rate,
                                mode: #bus_mode,
                            },
                        )
                        .expect("SPI bus configuration failed");
                    },
                )
            }
            None => cx.once(
                &key,
                quote! {
                    let #handle = espforge_runtime::SpiBus::build_write_only(
                        #peripheral, #sclk, #mosi,
                        espforge_runtime::SpiConfig {
                            frequency: #rate,
                            mode: #bus_mode,
                        },
                    )
                    .expect("SPI bus configuration failed");
                },
            ),
        };
        if inserted {
            let mut pins = format!("GPIO{sclk_pin}/GPIO{mosi_pin}");
            if let Some(miso_pin) = miso_pin {
                pins.push_str(&format!("/GPIO{miso_pin}"));
            }
            cx.safety_block(
                format!("let {handle} ="),
                &[
                    "SAFETY: espforge checked that no other component uses this",
                    &format!("peripheral or {pins}."),
                ],
            );
            if miso_pin.is_none() {
                cx.safety_comment(
                    format!("let {handle} ="),
                    "No miso: a write-only bus (displays).",
                );
            }
        }

        let name = format_ident!("{}", self.id.rust_name());
        let cs_pin = self.cs.pin().0;
        let cs = gpio(cs_pin);
        let runtime = runtime_type(board.meta.runtime);
        // A device that takes the bus (a display) owns this SPI device, so the
        // component's field is an `Option` that `Devices::new` takes. Without
        // one the field is plain, as it has always been.
        let value = quote! {
            espforge_runtime::SpiDevice::new(
                #handle,
                esp_hal::gpio::Output::new(
                    #cs,
                    esp_hal::gpio::Level::High,
                    esp_hal::gpio::OutputConfig::default(),
                ),
                espforge_runtime::Delay::new(),
            )
        };
        if board.takes_component(&self.id) {
            cx.field(quote! {
                pub #name: Option<espforge_runtime::SpiDevice<#runtime>>,
            });
            cx.init(quote! {
                #name: Some(#value),
            });
        } else {
            cx.field(quote! {
                pub #name: espforge_runtime::SpiDevice<#runtime>,
            });
            cx.init(quote! {
                #name: #value,
            });
        }
        // A consumed bus wraps the same constructor in `Some(..)`.
        let anchor = if board.takes_component(&self.id) {
            format!("{name}: Some(")
        } else {
            format!("{name}: espforge_runtime::SpiDevice::new(")
        };
        cx.safety_block(
            anchor,
            &[
                &format!("SAFETY: espforge checked that no other component uses GPIO{cs_pin}."),
                "Chip-select idles high (deasserted).",
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Console, Meta, Pin, PinUse, ResourceGraph, Runtime};

    fn board() -> Board {
        Board {
            meta: Meta {
                name: "spi".into(),
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

    /// A bus with the given MISO, plus `devices` chip selects on it.
    fn bus_with(miso: bool, devices: &[(u8, &str)]) -> (Board, Vec<SpiComponent>) {
        let mut board = board();
        let owner = Owner::new(OwnerKind::Spi, Ident::new("spi2", 0..0));
        let claim = |board: &mut Board, pin: u8| {
            board
                .resources
                .claim(Pin(pin), owner.clone(), 0..0, PinUse::Bidirectional)
                .expect("the test pins are free")
        };
        let sclk = claim(&mut board, 8);
        let mosi = claim(&mut board, 7);
        let miso = miso.then(|| claim(&mut board, 6));
        let bus = board.resources.add_bus(Bus::Spi {
            id: Ident::new("spi2", 0..0),
            peripheral: "SPI2".into(),
            sclk,
            mosi,
            miso,
            khz: 100,
            mode: 0,
        });
        let components = devices
            .iter()
            .map(|(pin, id)| {
                let cs = board
                    .resources
                    .claim(
                        Pin(*pin),
                        Owner::new(OwnerKind::SpiComponent, Ident::new(*id, 0..0)),
                        0..0,
                        PinUse::Unspecified,
                    )
                    .expect("the cs pins are free");
                SpiComponent {
                    id: Ident::new(*id, 0..0),
                    bus,
                    cs,
                    mode: 0,
                }
            })
            .collect();
        (board, components)
    }

    #[test]
    fn two_devices_on_one_bus_emit_one_bus_let() {
        let (board, components) = bus_with(true, &[(9, "a"), (10, "b")]);
        let mut cx = EmitCx::new();
        for component in &components {
            component.emit(&board, &mut cx);
        }
        let parts = cx.into_parts();
        assert_eq!(parts.prelude.len(), 1);
        assert_eq!(parts.fields.len(), 2);
        assert_eq!(parts.inits.len(), 2);
        // The safety note is emitted once, with the prelude.
        assert_eq!(
            parts
                .safety
                .iter()
                .filter(|(anchor, _)| anchor == "let spi2_bus =")
                .count(),
            1
        );
    }

    #[test]
    fn a_missing_miso_builds_a_write_only_bus() {
        let (board, components) = bus_with(false, &[(9, "display")]);
        let mut cx = EmitCx::new();
        components[0].emit(&board, &mut cx);
        let parts = cx.into_parts();
        let prelude = parts.prelude[0].to_string();
        assert!(prelude.contains("build_write_only"), "{prelude}");
        assert!(
            parts
                .safety
                .iter()
                .any(|(_, lines)| lines.iter().any(|l| l.contains("No miso"))),
            "the write-only bus says so"
        );
    }
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    match issue {
        Issue::Missing { attr } => cx.report(
            codes::COMPONENT_INVALID_SPI,
            node_span(node),
            "invalid spi_component",
            format!("❌ [spi_component] '{id}' missing required attribute '{attr}'"),
        ),
        Issue::Invalid { attr, value } => cx.report(
            codes::COMPONENT_INVALID_SPI,
            node_span(node),
            "invalid spi_component",
            format!("❌ [spi_component] '{id}' invalid {attr} '{value}': expected 0..=3"),
        ),
    }
}

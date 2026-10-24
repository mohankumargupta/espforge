//! Lowering into the typed IR (`Board`).
//!
//! Phase 2 increment 3: this path builds a [`Board`] from the parsed sections,
//! claiming every pin through [`ResourceGraph::claim`] instead of checking
//! ownership in a later pass. Validation of attributes and `$` references
//! still goes through the Phase 1 generated `Attrs` and the typed wrappers;
//! increment 4 moves those last checks here and deletes the old lowering.
//!
//! The old path stays authoritative until the dual-run tests and the
//! projection agree for every valid spec. Until then this module is exercised
//! by `tests/board_dual_run.rs`, not by `check`/`build`.
//!
//! "Only a valid board can exist" is enforced as far as the crate split
//! allows today: fields are public so the emitter's projection can live in
//! `espforge-model`, and `build` is the only production constructor. Phase 4's
//! `espforge-core` move puts `Board` and this function in one crate, where the
//! fields can become private without a second constructor.

use std::collections::{HashMap, HashSet};

use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::{EspfError, node_span};
use espforge_bindings::components::attrs::PinOrRef;
use espforge_bindings::components::{
    ButtonComponent as ButtonNode, I2cComponent as I2cNode, LedComponent as LedNode,
    Parity as SchemaParity, SpiComponent as SpiNode, StopBits as SchemaStopBits,
    UartComponent as UartNode,
};
use espforge_model::Meta;
use espforge_model::board::{
    Board, Bus, BusId, Button, ClaimedPin, Component, Conflict, ConflictKind, Framing,
    I2cComponent, Ident, Led, Owner, OwnerKind, Parity, PeripheralClash, Pin, PinUse,
    ResourceGraph, Service, SpiComponent, StopBits, UartComponent, WifiService,
};

use crate::devices;
use crate::peripherals::gpio::{GpioEntry, gpio_for_handle};
use crate::peripherals::{gpio, i2c, spi, uart};
use crate::services::wifi;

/// Build a `Board` from the parsed sections.
///
/// The sections must already have gone through
/// [`RuntimeContext::resolve_and_bind`], the same contract the old `lower`
/// has. Peripherals are built first (in source order), then components, then
/// services; pins are claimed as each element is built, so a conflict is an
/// error at the element that would have owned it.
pub fn build(
    sections: &HashMap<String, Vec<ParsedNode>>,
    ctx: &RuntimeContext,
    meta: &Meta,
) -> Result<Board, Vec<EspfError>> {
    devices::reject_if_present(sections)?;

    let mut graph = ResourceGraph::new();
    let mut gpios: Vec<GpioEntry> = Vec::new();
    let mut bus_ids: HashMap<String, BusId> = HashMap::new();
    let mut components: Vec<Component> = Vec::new();
    let mut cs_gpios: HashSet<String> = HashSet::new();

    for n in sections
        .get("peripherals")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        match n.tag_name {
            "gpio" => gpios.push(gpio::lower_gpio(n).map_err(|e| vec![e])?),
            "i2c" => {
                let draft = i2c::lower_i2c(n).map_err(|e| vec![e])?;
                let owner = Owner::new(OwnerKind::I2c, ident(n));
                let span = node_span(n);
                let sda = claim_pin(
                    &mut graph,
                    draft.sda,
                    owner.clone(),
                    span.clone(),
                    PinUse::Bidirectional,
                )?;
                let scl = claim_pin(&mut graph, draft.scl, owner, span, PinUse::Bidirectional)?;
                let bus = Bus::I2c {
                    id: ident(n),
                    peripheral: draft.peripheral,
                    sda,
                    scl,
                    khz: draft.khz,
                    used_by: None,
                };
                bus_ids.insert(n.id.to_string(), graph.add_bus(bus));
            }
            "spi" => {
                let draft = spi::lower_spi(n).map_err(|e| vec![e])?;
                let owner = Owner::new(OwnerKind::Spi, ident(n));
                let span = node_span(n);
                let sclk = claim_pin(
                    &mut graph,
                    draft.sclk,
                    owner.clone(),
                    span.clone(),
                    PinUse::Bidirectional,
                )?;
                let mosi = claim_pin(
                    &mut graph,
                    draft.mosi,
                    owner.clone(),
                    span.clone(),
                    PinUse::Bidirectional,
                )?;
                let miso = match draft.miso {
                    Some(pin) => Some(claim_pin(
                        &mut graph,
                        pin,
                        owner,
                        span,
                        PinUse::Bidirectional,
                    )?),
                    None => None,
                };
                let bus = Bus::Spi {
                    id: ident(n),
                    peripheral: "SPI2".to_string(),
                    sclk,
                    mosi,
                    miso,
                    khz: draft.khz,
                    mode: draft.mode,
                };
                bus_ids.insert(n.id.to_string(), graph.add_bus(bus));
            }
            "uart" => {
                let draft = uart::lower_uart(n).map_err(|e| vec![e])?;
                let owner = Owner::new(OwnerKind::Uart, ident(n));
                let span = node_span(n);
                graph
                    .claim_peripheral(&draft.peripheral, owner.clone(), span.clone())
                    .map_err(|clash| vec![peripheral_error(clash)])?;
                let tx = claim_pin(
                    &mut graph,
                    draft.tx,
                    owner.clone(),
                    span.clone(),
                    PinUse::Unspecified,
                )?;
                let rx = claim_pin(&mut graph, draft.rx, owner, span, PinUse::Unspecified)?;
                let bus = Bus::Uart {
                    id: ident(n),
                    peripheral: draft.peripheral,
                    tx,
                    rx,
                    used_by: None,
                };
                bus_ids.insert(n.id.to_string(), graph.add_bus(bus));
            }
            other => {
                return Err(vec![EspfError::at_node(
                    n,
                    "unsupported peripheral",
                    format!(
                        "<{other}> in `# peripherals` is not supported by `build` yet (only <gpio>, <i2c>, <spi> and <uart>)"
                    ),
                )]);
            }
        }
    }

    for n in sections.get("components").map(Vec::as_slice).unwrap_or(&[]) {
        match n.tag_name {
            "led" => {
                let node = LedNode::resolve(n, ctx)
                    .map_err(|m| vec![EspfError::at_node(n, "invalid led", m)])?;
                let gpio = gpio_for_handle(&gpios, ctx, node.peripheral_handle, n, node.gpio_ref)
                    .map_err(|e| vec![e])?;
                if let Some(direction) = gpio.direction.as_deref() {
                    if direction != "output" {
                        return Err(vec![EspfError::at_node(
                            n,
                            "wrong gpio direction",
                            format!(
                                "<led id=\"{}\"> needs an output pin, but `{}` has direction=\"{}\"",
                                n.id, gpio.id, direction
                            ),
                        )]);
                    }
                }
                let pin = claim_pin(
                    &mut graph,
                    gpio.pin,
                    Owner::new(OwnerKind::Led, ident(n)),
                    node_span(n),
                    PinUse::Output,
                )?;
                components.push(Component::Led(Led {
                    id: ident(n),
                    pin,
                    active_low: node.active_low,
                }));
            }
            "button" => {
                let node = ButtonNode::resolve(n, ctx)
                    .map_err(|m| vec![EspfError::at_node(n, "invalid button", m)])?;
                let gpio = gpio_for_handle(&gpios, ctx, node.peripheral_handle, n, node.gpio_ref)
                    .map_err(|e| vec![e])?;
                if let Some(direction) = gpio.direction.as_deref() {
                    if direction != "input" {
                        return Err(vec![EspfError::at_node(
                            n,
                            "wrong gpio direction",
                            format!(
                                "<button id=\"{}\"> needs an input pin, but `{}` has direction=\"{}\"",
                                n.id, gpio.id, direction
                            ),
                        )]);
                    }
                }
                let pin = claim_pin(
                    &mut graph,
                    gpio.pin,
                    Owner::new(OwnerKind::Button, ident(n)),
                    node_span(n),
                    PinUse::Input,
                )?;
                components.push(Component::Button(Button {
                    id: ident(n),
                    pin,
                    pull_up: node.pull_up,
                }));
            }
            "i2c_component" => {
                let node = I2cNode::resolve(n, ctx)
                    .map_err(|m| vec![EspfError::at_node(n, "invalid i2c_component", m)])?;
                let bus = bus_id_for(&bus_ids, node.i2c_ref, n, "i2c")?;
                if let Some(other) = used_by(&graph, bus) {
                    return Err(vec![EspfError::at_node(
                        n,
                        "i2c bus already used",
                        format!(
                            "<i2c_component id=\"{}\"> and <i2c_component id=\"{}\"> both use <i2c id=\"{}\">",
                            other.rust_name(),
                            n.id,
                            graph.bus(bus).map_or("", |bus| bus.id().raw.as_str())
                        ),
                    )]);
                }
                set_used_by(&mut graph, bus, ident(n));
                components.push(Component::I2c(I2cComponent {
                    id: ident(n),
                    bus,
                    address: node.address,
                }));
            }
            "spi_component" => {
                let node = SpiNode::resolve(n, ctx)
                    .map_err(|m| vec![EspfError::at_node(n, "invalid spi_component", m)])?;
                let bus = bus_id_for(&bus_ids, node.spi_ref, n, "spi")?;
                let (cs_pin, cs_gpio) =
                    match node.cs {
                        PinOrRef::Pin(pin) => (pin.value, None),
                        PinOrRef::Ref(reference) => {
                            let gpio = gpios.iter().find(|g| g.id == reference.name).ok_or_else(
                                || {
                                    vec![EspfError::at_node(
                                        n,
                                        "invalid cs",
                                        format!(
                                            "<spi_component id=\"{}\"> needs a numeric cs=\"...\"",
                                            n.id
                                        ),
                                    )]
                                },
                            )?;
                            (gpio.pin, Some(gpio.id.clone()))
                        }
                    };
                if let Some(gpio) = cs_gpio {
                    cs_gpios.insert(gpio);
                }
                let cs = claim_pin(
                    &mut graph,
                    cs_pin,
                    Owner::new(OwnerKind::SpiComponent, ident(n)),
                    node_span(n),
                    PinUse::Unspecified,
                )?;
                components.push(Component::Spi(SpiComponent {
                    id: ident(n),
                    bus,
                    cs,
                    mode: node.mode as u8,
                }));
            }
            "uart_component" => {
                let node = UartNode::resolve(n, ctx)
                    .map_err(|m| vec![EspfError::at_node(n, "invalid uart_component", m)])?;
                let bus = bus_id_for(&bus_ids, node.uart_ref, n, "uart")?;
                if let Some(other) = used_by(&graph, bus) {
                    return Err(vec![EspfError::at_node(
                        n,
                        "uart already used",
                        format!(
                            "<uart_component id=\"{}\"> and <uart_component id=\"{}\"> both use <uart id=\"{}\">",
                            other.rust_name(),
                            n.id.replace('-', "_"),
                            graph.bus(bus).map_or("", |bus| bus.id().raw.as_str())
                        ),
                    )]);
                }
                if node.baud_rate == 0 {
                    return Err(vec![EspfError::at_node(
                        n,
                        "invalid baud_rate",
                        format!(
                            "<uart_component id=\"{}\"> baud_rate=\"0\" must be greater than 0",
                            n.id
                        ),
                    )]);
                }
                set_used_by(&mut graph, bus, ident(n));
                components.push(Component::Uart(UartComponent {
                    id: ident(n),
                    port: bus,
                    baud: node.baud_rate,
                    framing: Framing {
                        parity: ir_parity(node.parity),
                        stop_bits: ir_stop_bits(node.stop_bits),
                    },
                }));
            }
            other => {
                return Err(vec![EspfError::at_node(
                    n,
                    "unsupported component",
                    format!(
                        "<{other}> in `# components` is not supported by `build` yet (only <led>, <button>, <i2c_component>, <spi_component> and <uart_component>)"
                    ),
                )]);
            }
        }
    }

    let services =
        match wifi::lower_services(sections.get("services").map(Vec::as_slice).unwrap_or(&[]))? {
            Some(wifi) => vec![Service::Wifi(WifiService {
                ssid: wifi.ssid,
                password: wifi.password,
            })],
            None => Vec::new(),
        };

    check_gpio_declarations(sections, &gpios, &graph, &components, &cs_gpios)?;

    Ok(Board {
        meta: meta.clone(),
        resources: graph,
        components,
        services,
    })
}

/// The `<gpio>` id plus its value span. The id is validated by the caller.
fn ident(n: &ParsedNode) -> Ident {
    let span = n
        .get_attr_spanned("id")
        .map(|attr| attr.val_offset..attr.val_offset + attr.val.len())
        .unwrap_or(n.offset..n.offset);
    Ident::new(n.id, span)
}

/// Claim one pin, turning the graph's structured conflict into a diagnostic.
fn claim_pin(
    graph: &mut ResourceGraph,
    pin: u8,
    owner: Owner,
    span: espforge_model::board::Span,
    need: PinUse,
) -> Result<ClaimedPin, Vec<EspfError>> {
    graph
        .claim(Pin(pin), owner, span, need)
        .map_err(|conflict| vec![conflict_error(conflict)])
}

/// Look up the `BusId` a component's `$` reference names.
fn bus_id_for(
    bus_ids: &HashMap<String, BusId>,
    reference: &str,
    n: &ParsedNode,
    tag: &str,
) -> Result<BusId, Vec<EspfError>> {
    let name = reference.trim_start_matches('$');
    bus_ids.get(name).copied().ok_or_else(|| {
        vec![EspfError::at_node(
            n,
            format!("unknown {tag} bus"),
            format!(
                "<{} id=\"{}\"> refers to `{name}`, which is not an <{tag}> in `# peripherals`",
                n.tag_name, n.id
            ),
        )]
    })
}

/// The component riding on a bus, if any.
fn used_by(graph: &ResourceGraph, bus: BusId) -> Option<Ident> {
    match graph.bus(bus) {
        Some(Bus::I2c { used_by, .. }) | Some(Bus::Uart { used_by, .. }) => used_by.clone(),
        _ => None,
    }
}

/// Record the component that rides on a bus.
fn set_used_by(graph: &mut ResourceGraph, bus: BusId, id: Ident) {
    match graph.bus_mut(bus) {
        Some(Bus::I2c { used_by, .. }) | Some(Bus::Uart { used_by, .. }) => *used_by = Some(id),
        _ => {}
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

/// `<gpio>` is a pin declaration, not an owner: a led or button that uses it
/// claims the pin itself. A bus, or someone else's chip select, sharing the
/// declared pin is still a wiring error, and this is where it is caught.
fn check_gpio_declarations(
    sections: &HashMap<String, Vec<ParsedNode>>,
    gpios: &[GpioEntry],
    graph: &ResourceGraph,
    components: &[Component],
    cs_gpios: &HashSet<String>,
) -> Result<(), Vec<EspfError>> {
    for bus in graph.buses() {
        match bus {
            Bus::I2c { id, sda, scl, .. } => {
                for gpio in gpios {
                    if gpio.pin == sda.pin().0 || gpio.pin == scl.pin().0 {
                        return Err(vec![gpio_conflict(sections, gpio, "i2c", &id.raw)]);
                    }
                }
            }
            Bus::Spi {
                id,
                sclk,
                mosi,
                miso,
                ..
            } => {
                for gpio in gpios {
                    let on_bus = gpio.pin == sclk.pin().0
                        || gpio.pin == mosi.pin().0
                        || miso.as_ref().is_some_and(|pin| pin.pin().0 == gpio.pin);
                    if on_bus {
                        return Err(vec![gpio_conflict(sections, gpio, "spi", &id.raw)]);
                    }
                }
            }
            Bus::Uart { id, tx, rx, .. } => {
                for gpio in gpios {
                    if gpio.pin == tx.pin().0 || gpio.pin == rx.pin().0 {
                        return Err(vec![gpio_conflict(sections, gpio, "uart", &id.raw)]);
                    }
                }
            }
        }
    }

    for component in components {
        if let Component::Spi(spi) = component {
            for gpio in gpios {
                if gpio.pin == spi.cs.pin().0 && !cs_gpios.contains(&gpio.id) {
                    return Err(vec![gpio_conflict(
                        sections,
                        gpio,
                        "spi_component",
                        &spi.id.raw,
                    )]);
                }
            }
        }
    }

    Ok(())
}

/// The old conflict wording, pointed at the `<gpio>` that declared the pin.
fn gpio_conflict(
    sections: &HashMap<String, Vec<ParsedNode>>,
    gpio: &GpioEntry,
    tag: &str,
    id: &str,
) -> EspfError {
    let label = format!(
        "<gpio id=\"{}\"> and <{tag} id=\"{id}\"> both use GPIO{}",
        gpio.id, gpio.pin
    );
    match sections.get("peripherals").and_then(|nodes| {
        nodes
            .iter()
            .find(|n| n.tag_name == "gpio" && n.id == gpio.id)
    }) {
        Some(node) => EspfError::at_node(node, "pin conflict", label),
        None => EspfError::at_span(0..0, "pin conflict", label),
    }
}

/// A refused claim in the wording users see today.
///
/// The blame order and primary span follow the old `conflicts.rs` loops for
/// the cases the corpus pins; the led/button direction clash now names the
/// real first owner (the old code used the incoming id twice, a known defect
/// recorded in `phase0-baseline.md`).
fn conflict_error(conflict: Conflict) -> EspfError {
    let pin = conflict.pin.0;
    let existing = &conflict.existing;
    let incoming = &conflict.incoming;

    let (title, label, primary) = match conflict.kind {
        ConflictKind::Direction => (
            "pin direction clash",
            format!(
                "{} and {}: one pin cannot be both input and output",
                display(&existing.owner, true),
                display(&incoming.owner, false)
            ),
            incoming.span.clone(),
        ),
        ConflictKind::SelfClash => match existing.owner.kind {
            OwnerKind::Uart => (
                "uart wiring",
                format!(
                    "{} tx and rx cannot both be GPIO{pin}",
                    display(&existing.owner, false)
                ),
                incoming.span.clone(),
            ),
            OwnerKind::I2c => (
                "i2c wiring",
                format!(
                    "{} sda and scl cannot both be GPIO{pin}",
                    display(&existing.owner, false)
                ),
                incoming.span.clone(),
            ),
            OwnerKind::Spi => (
                "spi wiring",
                format!(
                    "{} sclk and mosi cannot both be GPIO{pin}",
                    display(&existing.owner, false)
                ),
                incoming.span.clone(),
            ),
            _ => (
                "pin already used",
                format!("{} claims GPIO{pin} twice", display(&existing.owner, false)),
                incoming.span.clone(),
            ),
        },
        ConflictKind::Pin => pin_conflict(&conflict),
    };

    EspfError::at_span(primary, title, label)
}

/// The `Pin` case: both owners are different elements.
fn pin_conflict(conflict: &Conflict) -> (&'static str, String, espforge_model::board::Span) {
    let pin = conflict.pin.0;
    let existing = &conflict.existing;
    let incoming = &conflict.incoming;

    match (&existing.owner.kind, &incoming.owner.kind) {
        (OwnerKind::Led, OwnerKind::Led) | (OwnerKind::Button, OwnerKind::Button) => (
            "pin already used",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, true),
                display(&incoming.owner, false)
            ),
            incoming.span.clone(),
        ),
        (OwnerKind::SpiComponent, OwnerKind::SpiComponent) => (
            "cs pin already used",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, true),
                display(&incoming.owner, true)
            ),
            incoming.span.clone(),
        ),
        (OwnerKind::Spi, OwnerKind::SpiComponent) => (
            "pin conflict",
            format!(
                "{}'s cs and {} both use GPIO{pin}",
                display(&incoming.owner, false),
                display(&existing.owner, false)
            ),
            incoming.span.clone(),
        ),
        (OwnerKind::I2c, OwnerKind::Uart) | (OwnerKind::Uart, OwnerKind::I2c) => {
            let (i2c, uart) = if existing.owner.kind == OwnerKind::I2c {
                (existing, incoming)
            } else {
                (incoming, existing)
            };
            (
                "pin conflict",
                format!(
                    "{} and {} both use GPIO{pin}",
                    display(&i2c.owner, false),
                    display(&uart.owner, false)
                ),
                i2c.span.clone(),
            )
        }
        (_, OwnerKind::Led | OwnerKind::Button) => (
            "pin conflict",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&incoming.owner, true),
                display(&existing.owner, false)
            ),
            incoming.span.clone(),
        ),
        (OwnerKind::Led | OwnerKind::Button, _) => (
            "pin conflict",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, true),
                display(&incoming.owner, false)
            ),
            existing.span.clone(),
        ),
        _ => (
            "pin conflict",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, false),
                display(&incoming.owner, false)
            ),
            existing.span.clone(),
        ),
    }
}

/// One peripheral name claimed twice (`UART1`).
fn peripheral_error(clash: PeripheralClash) -> EspfError {
    EspfError::at_span(
        clash.existing.span.clone(),
        "peripheral clash",
        format!(
            "{} and {} both use {}",
            display(&clash.existing.owner, false),
            display(&clash.incoming.owner, false),
            clash.peripheral
        ),
    )
}

/// `<tag id="...">`; `slug` uses the Rust identifier, as the old conflict
/// labels did for the owner that was already lowered.
fn display(owner: &Owner, slug: bool) -> String {
    let id = if slug {
        owner.id.rust_name()
    } else {
        owner.id.raw.clone()
    };
    format!("<{} id=\"{id}\">", owner.kind.tag())
}

#[cfg(test)]
mod tests {
    use super::*;
    use espf::parser::parse_espf;
    use espforge_model::{Console, Runtime};

    fn meta() -> Meta {
        Meta {
            name: "board".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        }
    }

    fn build_src(src: &str) -> Result<Board, Vec<EspfError>> {
        let parsed = parse_espf(src);
        let mut ctx = RuntimeContext::new();
        ctx.resolve_and_bind(
            &parsed.sections["peripherals"],
            &parsed.sections["components"],
            &parsed.sections["devices"],
        )?;
        build(&parsed.sections, &ctx, &meta())
    }

    const LED_BUTTON: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" active_low=\"true\" />\n  <button id=\"btn\" gpio=\"$gpio9\" pull_up=\"true\" />\n# devices\n# services\n";

    #[test]
    fn leds_and_buttons_become_claimed_components() {
        let board = build_src(LED_BUTTON).unwrap();
        assert_eq!(board.components.len(), 2);
        assert_eq!(board.components[0].id().raw, "red-led");
        assert_eq!(board.components[1].id().raw, "btn");
        assert_eq!(
            board.hardware(),
            crate::Hardware {
                leds: vec![crate::LedSpec {
                    name: "red_led".into(),
                    pin: 18,
                    active_low: true,
                }],
                buttons: vec![crate::ButtonSpec {
                    name: "btn".into(),
                    pin: 9,
                    pull_up: true,
                }],
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_led_and_a_button_on_one_pin_are_a_direction_clash() {
        // Two gpio declarations on the same physical pin, one output for the
        // led and one input for the button: the pins are settled first, so
        // this is the claim's direction rule, not the declaration check.
        let src = LED_BUTTON.replace(
            "<gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />",
            "<gpio id=\"gpio9\" pin=\"18\" direction=\"input\" />",
        );
        let err = build_src(&src).unwrap_err();
        assert_eq!(err[0].title, "pin direction clash");
        assert!(
            err[0].label.contains("<led id=\"red_led\">"),
            "{}",
            err[0].label
        );
        assert!(
            err[0].label.contains("<button id=\"btn\">"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn a_bus_and_a_gpio_on_one_pin_are_a_conflict_at_the_gpio() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"g\" pin=\"4\" />\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "pin conflict");
        assert!(
            err[0]
                .label
                .contains("<gpio id=\"g\"> and <i2c id=\"i2c0\">"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn i2c_spi_and_uart_components_project_through() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" frequency=\"400kHz\" />\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"1000kHz\" mode=\"3\" />\n  <uart id=\"uart1\" tx=\"20\" rx=\"21\" />\n# components\n  <i2c_component id=\"oled\" i2c=\"$i2c0\" address=\"0x3c\" />\n  <spi_component id=\"display\" spi=\"$spi2\" cs=\"9\" />\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"115200\" parity=\"even\" stop_bits=\"2\" />\n# devices\n# services\n";
        let board = build_src(src).unwrap();
        assert_eq!(board.components.len(), 3);
        assert_eq!(board.resources.buses().len(), 3);

        let hardware = board.hardware();
        assert_eq!(hardware.i2cs[0].name, "oled");
        assert_eq!(hardware.i2cs[0].peripheral, "I2C0");
        assert_eq!(hardware.spis[0].name, "display");
        assert_eq!(hardware.spis[0].bus, "spi2");
        assert_eq!(hardware.uarts[0].name, "serial");
        assert_eq!(hardware.uarts[0].parity, "Even");
        assert_eq!(hardware.uarts[0].stop_bits, "_2");
    }

    #[test]
    fn a_cs_may_name_a_gpio_and_the_declaration_is_not_a_conflict() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" />\n# components\n  <spi_component id=\"display\" spi=\"$spi2\" cs=\"gpio9\" />\n# devices\n# services\n";
        let board = build_src(src).unwrap();
        assert_eq!(board.hardware().spis[0].cs, 9);
    }

    #[test]
    fn two_leds_on_one_pin_keep_the_legacy_title_and_label() {
        let src = LED_BUTTON.replace(
            "<button id=\"btn\" gpio=\"$gpio9\" pull_up=\"true\" />",
            "<led id=\"other\" gpio=\"$gpio18\" />",
        );
        let err = build_src(&src).unwrap_err();
        assert_eq!(err[0].title, "pin already used");
        assert!(
            err[0]
                .label
                .contains("<led id=\"red_led\"> and <led id=\"other\"> both use GPIO18"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn uart_tx_and_rx_on_one_pin_is_uart_wiring() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"5\" rx=\"5\" />\n# components\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "uart wiring");
        assert!(
            err[0]
                .label
                .contains("<uart id=\"uart1\"> tx and rx cannot both be GPIO5"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn two_components_on_one_i2c_bus_are_refused() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"a\" i2c=\"$i2c0\" address=\"0x48\" />\n  <i2c_component id=\"b\" i2c=\"$i2c0\" address=\"0x49\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "i2c bus already used");
        assert!(
            err[0].label.contains("both use <i2c id=\"i2c0\">"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn two_uarts_on_one_peripheral_are_a_peripheral_clash() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"gps\" uart=\"UART1\" tx=\"20\" rx=\"21\" />\n  <uart id=\"other\" uart=\"UART1\" tx=\"22\" rx=\"23\" />\n# components\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "peripheral clash");
        assert!(err[0].label.contains("both use UART1"), "{}", err[0].label);
    }

    #[test]
    fn a_zero_baud_rate_is_refused() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"20\" rx=\"21\" />\n# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"0\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "invalid baud_rate");
    }
}

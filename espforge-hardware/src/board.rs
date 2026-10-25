//! Lowering into the typed IR (`Board`).
//!
//! Phase 2 increment 4: `check` and `build` come through here. The `$` graph is
//! resolved by [`espf::context::Refs`], attributes are the Phase 1 generated
//! `Attrs`, and every pin is claimed through [`ResourceGraph::claim`] as the
//! element is built instead of being checked in a later pass. This is the only
//! lowering path: the handle-based graph, the typed wrappers, the drafts and
//! the post-hoc conflict pass are gone.
//!
//! "Only a valid board can exist" is enforced as far as the crate split
//! allows today: fields are public so the emitter's projection can live in
//! `espforge-model`, and `build` is the only production constructor. Phase 4's
//! `espforge-core` move puts `Board` and this function in one crate, where the
//! fields can become private without a second constructor.

use std::collections::{HashMap, HashSet};

use espf::context::{ParsedNode, PeripheralKind, Refs};
use espf::diag::{EspfError, node_span};
use espforge_bindings::components::attrs::{Issue, PinOrRef};
use espforge_bindings::components::generated_elements::uart_component::{
    Parity as SchemaParity, StopBits as SchemaStopBits,
};
use espforge_bindings::components::generated_elements::{
    button as button_schema, i2c_component as i2c_schema, led as led_schema,
    spi_component as spi_schema, uart_component as uart_schema,
};
use espforge_model::Meta;
use espforge_model::board::{
    Board, Bus, BusId, Button, ClaimedPin, Component, Conflict, ConflictKind, Framing,
    I2cComponent, Ident, Led, Owner, OwnerKind, Parity, PeripheralClash, Pin, PinUse,
    ResourceGraph, Service, SpiComponent, StopBits, UartComponent,
};

use crate::devices;
use crate::peripherals::gpio::GpioEntry;
use crate::peripherals::{gpio, i2c, spi, uart};
use crate::services::wifi;
use crate::util::is_rust_ident;

/// Build a `Board` from the parsed sections.
///
/// The `$` graph is resolved here first, then peripherals come in source order,
/// then components, then services. Pins are claimed as each element is built,
/// so a conflict is an error at the element that would have owned it.
pub fn build(
    sections: &HashMap<String, Vec<ParsedNode>>,
    meta: &Meta,
) -> Result<Board, Vec<EspfError>> {
    let refs = Refs::resolve(sections)?;
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
    } // A bus pin claimed by a `<gpio>` declaration is an error before any
    // component is built, which is the order the old conflict pass used:
    // the corpus pins `conflict-spi-vs-gpio` at the gpio, not at the led that
    // happens to use the same pin.
    check_bus_gpio_declarations(sections, &gpios, &graph)?;

    for n in sections.get("components").map(Vec::as_slice).unwrap_or(&[]) {
        match n.tag_name {
            "led" => {
                let (active_low, gpio_name) = led_node(n, &refs)?;
                let gpio = find_gpio(&gpios, n, gpio_name)?;
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
                    active_low,
                }));
            }
            "button" => {
                let (pull_up, gpio_name) = button_node(n, &refs)?;
                let gpio = find_gpio(&gpios, n, gpio_name)?;
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
                    pull_up,
                }));
            }
            "i2c_component" => {
                let (address, bus_name) = i2c_node(n, &refs)?;
                let bus = bus_id_for(&bus_ids, bus_name, n, "i2c")?;
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
                    address,
                }));
            }
            "spi_component" => {
                let (mode, cs, bus_name) = spi_node(n, &refs)?;
                let bus = bus_id_for(&bus_ids, bus_name, n, "spi")?;
                let (cs_pin, cs_gpio) =
                    match cs {
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
                    mode: mode as u8,
                }));
            }
            "uart_component" => {
                let (baud_rate, parity, stop_bits, bus_name) = uart_node(n, &refs)?;
                let bus = bus_id_for(&bus_ids, bus_name, n, "uart")?;
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
                if baud_rate == 0 {
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
                    baud: baud_rate,
                    framing: Framing {
                        parity: ir_parity(parity),
                        stop_bits: ir_stop_bits(stop_bits),
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
            Some(wifi) => vec![Service::Wifi(wifi)],
            None => Vec::new(),
        };

    check_spi_cs_gpio_declarations(sections, &gpios, &components, &cs_gpios)?;

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
/// `<gpio>` is a pin declaration, not an owner: a led or button that uses it
/// claims the pin itself. A bus sharing the declared pin is still a wiring
/// error, and this is where it is caught (before any component claim, so the
/// error points at the gpio as the old pass did).
fn check_bus_gpio_declarations(
    sections: &HashMap<String, Vec<ParsedNode>>,
    gpios: &[GpioEntry],
    graph: &ResourceGraph,
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

    Ok(())
}

/// A chip select written as a pin number that collides with a declared
/// `<gpio>`: the gpio is a second user of the pin unless the chip select named
/// that gpio itself.
fn check_spi_cs_gpio_declarations(
    sections: &HashMap<String, Vec<ParsedNode>>,
    gpios: &[GpioEntry],
    components: &[Component],
    cs_gpios: &HashSet<String>,
) -> Result<(), Vec<EspfError>> {
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

// ------------------------------------------------- component attributes

/// Resolve `<led>`: schema attributes, the `$gpio` kind and the slug rule.
fn led_node<'a>(n: &'a ParsedNode<'a>, refs: &Refs) -> Result<(bool, &'a str), Vec<EspfError>> {
    let attrs = led_schema::Attrs::from_node(n)
        .map_err(|issue| vec![EspfError::at_node(n, "invalid led", led_issue(n.id, issue))])?;
    let name = attrs.gpio.name;
    if refs.kind(name) != Some(PeripheralKind::Gpio) {
        return Err(vec![EspfError::at_node(
            n,
            "invalid led",
            format!(
                "<led id=\"{}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`",
                n.id
            ),
        )]);
    }
    require_slug(n, "invalid led", "led")?;
    Ok((attrs.active_low.value, name))
}

/// Resolve `<button>`: schema attributes, the `$gpio` kind and the slug rule.
fn button_node<'a>(n: &'a ParsedNode<'a>, refs: &Refs) -> Result<(bool, &'a str), Vec<EspfError>> {
    let attrs = button_schema::Attrs::from_node(n).map_err(|issue| {
        vec![EspfError::at_node(
            n,
            "invalid button",
            button_issue(n.id, issue),
        )]
    })?;
    let name = attrs.gpio.name;
    if refs.kind(name) != Some(PeripheralKind::Gpio) {
        return Err(vec![EspfError::at_node(
            n,
            "invalid button",
            format!(
                "<button id=\"{}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`",
                n.id
            ),
        )]);
    }
    require_slug(n, "invalid button", "button")?;
    Ok((attrs.pull_up.value, name))
}

/// Resolve `<i2c_component>`: schema attributes, the `$i2c` kind and the slug.
fn i2c_node<'a>(
    n: &'a ParsedNode<'a>,
    refs: &Refs,
) -> Result<(Option<u8>, &'a str), Vec<EspfError>> {
    let attrs = i2c_schema::Attrs::from_node(n).map_err(|issue| {
        vec![EspfError::at_node(
            n,
            "invalid i2c_component",
            i2c_issue(n.id, issue),
        )]
    })?;
    let name = attrs.i2c.name;
    require_bus(
        n,
        refs,
        name,
        PeripheralKind::I2c,
        "i2c_component",
        "i2c",
        "an i2c",
        "non-I2C",
    )?;
    require_slug(n, "invalid i2c_component", "i2c_component")?;
    Ok((attrs.address.map(|address| address.value), name))
}

/// Resolve `<spi_component>`: schema attrs, the `cs` ref and the `$spi` kind.
fn spi_node<'a>(
    n: &'a ParsedNode<'a>,
    refs: &Refs,
) -> Result<(u32, PinOrRef<'a>, &'a str), Vec<EspfError>> {
    let attrs = spi_schema::Attrs::from_node(n).map_err(|issue| {
        vec![EspfError::at_node(
            n,
            "invalid spi_component",
            spi_issue(n.id, issue),
        )]
    })?;
    if let PinOrRef::Ref(reference) = attrs.cs {
        if !refs.knows(reference.name) {
            return Err(vec![EspfError::at_node(
                n,
                "invalid spi_component",
                format!(
                    "❌ [spi_component] '{}' cs references undefined gpio '{}'",
                    n.id, reference.name
                ),
            )]);
        }
    }
    let name = attrs.spi.name;
    require_bus(
        n,
        refs,
        name,
        PeripheralKind::Spi,
        "spi_component",
        "spi",
        "an spi",
        "non-SPI",
    )?;
    require_slug(n, "invalid spi_component", "spi_component")?;
    Ok((attrs.mode.value, attrs.cs, name))
}

/// Resolve `<uart_component>`: schema attrs, the `$uart` kind and the slug.
fn uart_node<'a>(
    n: &'a ParsedNode<'a>,
    refs: &Refs,
) -> Result<(u32, SchemaParity, SchemaStopBits, &'a str), Vec<EspfError>> {
    let attrs = uart_schema::Attrs::from_node(n).map_err(|issue| {
        vec![EspfError::at_node(
            n,
            "invalid uart_component",
            uart_issue(n.id, issue),
        )]
    })?;
    let name = attrs.uart.name;
    require_bus(
        n,
        refs,
        name,
        PeripheralKind::Uart,
        "uart_component",
        "uart",
        "a uart",
        "non-UART",
    )?;
    require_slug(n, "invalid uart_component", "uart_component")?;
    Ok((
        attrs.baud_rate.value,
        attrs.parity.value,
        attrs.stop_bits.value,
        name,
    ))
}

/// The reference wording users already know, kept byte-identical.
fn require_bus(
    n: &ParsedNode,
    refs: &Refs,
    name: &str,
    kind: PeripheralKind,
    label: &str,
    noun: &str,
    article: &str,
    non: &str,
) -> Result<(), Vec<EspfError>> {
    if !refs.knows(name) {
        return Err(vec![EspfError::at_node(
            n,
            format!("invalid {label}"),
            format!(
                "❌ [{label}] '{}' references undefined {noun} peripheral '{name}'",
                n.id
            ),
        )]);
    }
    match refs.kind(name) {
        None => Err(vec![EspfError::at_node(
            n,
            format!("invalid {label}"),
            format!(
                "❌ [{label}] '{}' {noun} peripheral '{name}' is not active",
                n.id
            ),
        )]),
        Some(found) if found != kind => Err(vec![EspfError::at_node(
            n,
            format!("invalid {label}"),
            format!(
                "❌ [{label}] '{}' expects {article} peripheral, got a {non} parent bus",
                n.id
            ),
        )]),
        _ => Ok(()),
    }
}

/// The Rust-identifier rule every component id must satisfy.
fn require_slug(n: &ParsedNode, title: &str, tag: &str) -> Result<(), Vec<EspfError>> {
    let slug = n.id.replace('-', "_");
    if is_rust_ident(&slug) {
        Ok(())
    } else {
        Err(vec![EspfError::at_node(
            n,
            title,
            format!(
                "<{tag} id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ),
        )])
    }
}

/// Locate the `<gpio>` entry a led/button names.
fn find_gpio<'a>(
    gpios: &'a [GpioEntry],
    n: &ParsedNode,
    name: &str,
) -> Result<&'a GpioEntry, Vec<EspfError>> {
    gpios.iter().find(|gpio| gpio.id == name).ok_or_else(|| {
        vec![EspfError::at_node(
            n,
            "unknown gpio",
            format!(
                "<{} id=\"{}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`",
                n.tag_name, n.id
            ),
        )]
    })
}

fn led_issue(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr: "gpio" } => {
            format!("<led id=\"{id}\"> needs gpio=\"$<gpio id>\"")
        }
        Issue::Missing { attr } => format!("Missing required attribute '{attr}'"),
        Issue::Invalid { attr, value } => {
            format!("<led id=\"{id}\"> {attr}=\"{value}\" must be \"true\" or \"false\"")
        }
    }
}

fn button_issue(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr: "gpio" } => {
            format!("<button id=\"{id}\"> needs gpio=\"$<gpio id>\"")
        }
        Issue::Missing { attr } => format!("Missing required attribute '{attr}'"),
        Issue::Invalid { attr, value } => {
            format!("<button id=\"{id}\"> {attr}=\"{value}\" must be \"true\" or \"false\"")
        }
    }
}

fn i2c_issue(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [i2c_component] Missing required attribute '{attr}'")
        }
        Issue::Missing { attr } => {
            format!("❌ [i2c_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid { value, .. } if value.starts_with("0x") || value.starts_with("0X") => {
            format!("❌ [i2c_component] '{id}' invalid hex address '{value}'")
        }
        Issue::Invalid { value, .. } => {
            format!("❌ [i2c_component] '{id}' invalid decimal address '{value}'")
        }
    }
}

fn spi_issue(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [spi_component] Missing required attribute '{attr}'")
        }
        Issue::Missing { attr } => {
            format!("❌ [spi_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid { attr, value } => {
            format!("❌ [spi_component] '{id}' invalid {attr} '{value}': expected 0..=3")
        }
    }
}

fn uart_issue(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [uart_component] Missing required attribute '{attr}'")
        }
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
    }
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
        build(&parsed.sections, &meta())
    }

    const LED_BUTTON: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" active_low=\"true\" />\n  <button id=\"btn\" gpio=\"$gpio9\" pull_up=\"true\" />\n# devices\n# services\n";

    #[test]
    fn leds_and_buttons_become_claimed_components() {
        let board = build_src(LED_BUTTON).unwrap();
        assert_eq!(board.components.len(), 2);
        assert_eq!(board.components[0].id().raw, "red-led");
        assert_eq!(board.components[1].id().raw, "btn");
        assert_eq!(board.components.len(), 2);
        let Component::Led(led) = &board.components[0] else {
            panic!("red-led is a led");
        };
        assert_eq!(
            (led.id.rust_name(), led.pin.pin().0, led.active_low),
            ("red_led".to_string(), 18, true)
        );
        let Component::Button(button) = &board.components[1] else {
            panic!("btn is a button");
        };
        assert_eq!((button.pin.pin().0, button.pull_up), (9, true));
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

        let ids: Vec<_> = board
            .components
            .iter()
            .map(|c| c.id().rust_name())
            .collect();
        assert_eq!(ids, vec!["oled", "display", "serial"]);
        let Component::I2c(oled) = &board.components[0] else {
            panic!("oled is an i2c component");
        };
        assert!(matches!(
            board.bus(oled.bus),
            Some(Bus::I2c { peripheral, .. }) if peripheral == "I2C0"
        ));
        let Component::Spi(display) = &board.components[1] else {
            panic!("display is an spi component");
        };
        assert_eq!(board.bus(display.bus).unwrap().id().raw, "spi2");
        let Component::Uart(serial) = &board.components[2] else {
            panic!("serial is a uart component");
        };
        assert_eq!(serial.framing.parity, Parity::Even);
        assert_eq!(serial.framing.stop_bits, StopBits::Two);
    }

    #[test]
    fn a_cs_may_name_a_gpio_and_the_declaration_is_not_a_conflict() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" />\n# components\n  <spi_component id=\"display\" spi=\"$spi2\" cs=\"gpio9\" />\n# devices\n# services\n";
        let board = build_src(src).unwrap();
        let Component::Spi(display) = &board.components[0] else {
            panic!("display is an spi component");
        };
        assert_eq!(display.cs.pin().0, 9);
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
    fn a_led_on_an_input_pin_is_refused() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"g\" pin=\"4\" direction=\"input\" />\n# components\n  <led id=\"l\" gpio=\"$g\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "wrong gpio direction");
        assert!(
            err[0].label.contains("needs an output pin"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn a_button_on_an_output_pin_is_refused() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"g\" pin=\"4\" direction=\"output\" />\n# components\n  <button id=\"b\" gpio=\"$g\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "wrong gpio direction");
        assert!(
            err[0].label.contains("needs an input pin"),
            "{}",
            err[0].label
        );
    }

    #[test]
    fn a_bad_i2c_address_keeps_the_wrapper_wording() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0xZZ\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "invalid i2c_component");
        assert_eq!(
            err[0].label,
            "❌ [i2c_component] 'c' invalid hex address '0xZZ'"
        );
    }

    #[test]
    fn an_i2c_component_may_omit_its_address() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"scanner\" i2c=\"$i2c0\" />\n# devices\n# services\n";
        let board = build_src(src).unwrap();
        let Component::I2c(component) = &board.components[0] else {
            panic!("expected an i2c component");
        };
        assert_eq!(component.address, None);
    }

    #[test]
    fn a_spi_cs_naming_an_undefined_gpio_is_refused() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" />\n# components\n  <spi_component id=\"d\" spi=\"$spi2\" cs=\"$gpioX\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "invalid spi_component");
        assert_eq!(
            err[0].label,
            "❌ [spi_component] 'd' cs references undefined gpio 'gpioX'"
        );
    }

    #[test]
    fn a_zero_baud_rate_is_refused() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"20\" rx=\"21\" />\n# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"0\" />\n# devices\n# services\n";
        let err = build_src(src).unwrap_err();
        assert_eq!(err[0].title, "invalid baud_rate");
    }
}

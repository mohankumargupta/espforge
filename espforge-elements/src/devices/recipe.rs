//! Generic device lowering: one interpreter for every `device_recipe`.
//!
//! A display and a sensor differ only in data the schema already carries: the
//! bus they attach to, the pins they claim, and the Rust expression
//! each allowed attribute spelling emits. [`DeviceRecipeElement`] reads that
//! data and follows the same steps a hand-written device element did. It knows
//! no tag.
//!
//! The typed, schema-generated `Attrs` readers still own presence and kind:
//! [`crate::generated::validate`] runs first, so a missing or misspelled value
//! is refused with the same wording as before, and an unmapped spelling cannot
//! reach emission. `espforge-schema` proves at load time that every value the
//! schema allows has a mapping.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use espforge_schema::{
    AddressNeed, CtorArg, DeviceArg, DeviceRecipe, MappedValue, PartSource, RecipePart, RustExpr,
    RustPath, RustType, TypeTemplate,
};
use std::collections::BTreeMap;

use crate::attrs;
use crate::devices::{invalid_element, invalid_id, span_of, unknown_gpio, wrong_direction};
use crate::element::{Element, LowerCx, Reported, Section, is_rust_ident};
use crate::generated;
use crate::model::{
    Bus, BusDevice, BusId, BusTransport, ClaimedPin, CsPolarity, Device, Direction, Driver,
    FactoryArg, I2cTransport, Ident, Init, Owner, OwnerKind, Pin, PinUse, PinsDevice, Runtime,
    SpiConfig as DeviceSpiConfig, SpiTransport, Step, UartTransport,
};

/// The interpreter for one schema element that declares a `device_recipe`.
///
/// It stores the tag, not the element: the element belongs to the session that
/// composed the schema and is looked up through the lowering context, so two
/// sessions can own two different definitions of the same tag.
pub struct DeviceRecipeElement {
    tag: String,
}

impl DeviceRecipeElement {
    /// An interpreter over the element `tag`, which must declare a
    /// `device_recipe`.
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Element for DeviceRecipeElement {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn lower<'s>(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'s>) -> Result<(), Reported> {
        let tag = self.tag.as_str();
        let element = cx
            .schema()
            .element(tag)
            .unwrap_or_else(|| panic!("`<{tag}>` is not in the schema"));
        let recipe = element
            .device_recipe
            .as_ref()
            .expect("a DeviceRecipeElement is only built for an element with a device_recipe");

        // 1. the schema's own readers: presence, kind and allowed values.
        match generated::validate(tag, node) {
            Some(Err(issue)) => {
                return Err(invalid_element(cx, node, tag, element, issue));
            }
            Some(Ok(())) => {}
            // A tag loaded from a project manifest has no build-time reader:
            // validate it from the schema the element carries.
            None => {
                crate::dynamic_attrs::validate(element, node)
                    .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
            }
        }

        // 2. the id must become a Rust identifier.
        let id = cx.checked_ident(node, Section::Devices)?;
        if !is_rust_ident(&id.rust_name()) {
            return Err(invalid_id(cx, node, tag, &id));
        }

        // 3. the composite's own device.
        let device = lower_recipe(cx, node, tag, element, recipe, &id, &Bindings::none())?;
        cx.push_device(device);

        // 4. one device per composed part, on the composite's bus. A part's
        // attributes come from the composite's own (`from=`) or are fixed by
        // the manifest (`value=`), so the spec never writes the part.
        for part in &recipe.parts {
            let part_element = cx
                .schema()
                .element(&part.tag)
                .unwrap_or_else(|| panic!("`<{}>` is not in the schema", part.tag));
            let part_recipe = part_element
                .device_recipe
                .as_ref()
                .expect("`compose` is checked against a device recipe at load");
            let part_id = Ident::new(
                format!("{}_{}", id.rust_name(), part.tag.replace('-', "_")),
                id.span.clone(),
            );
            if cx
                .devices()
                .iter()
                .any(|known| known.id().raw == part_id.raw)
            {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    id.span.clone(),
                    "duplicate device id",
                    format!(
                        "<{tag} id=\"{}\"> composes `<{}>`, whose device id `{}` is already taken",
                        id.raw, part.tag, part_id.raw
                    ),
                ));
            }
            let device = lower_recipe(
                cx,
                node,
                &part.tag,
                part_element,
                part_recipe,
                &part_id,
                &Bindings::of(part),
            )?;
            cx.push_device(device);
        }
        Ok(())
    }
}

/// Lower one recipe into the device it describes.
///
/// The composite and each of its parts go through this same function: the only
/// difference is [`Bindings`], which says where an attribute the spec does not
/// write comes from. The bus, the transport and the slots are read from the
/// recipe, so composition needs no second lowerer.
#[allow(clippy::too_many_arguments)]
fn lower_recipe<'s>(
    cx: &mut LowerCx<'s>,
    node: &ParsedNode<'_>,
    tag: &str,
    element: &'s espforge_schema::Element,
    element_recipe: &'s DeviceRecipe,
    id: &Ident,
    bindings: &Bindings<'s>,
) -> Result<Device, Reported> {
    // 3. the bus peripheral, of the kind the recipe declares.
    //
    // Whether the bus is blocking or async is a property of the bus, not
    // of the project: a blocking display on an embassy board is legal, so
    // the scheduler is no longer consulted here.
    // A busless driver (`bus "none"`) declares no `bus=` attribute: it owns the
    // pins it claims, so there is no peripheral to resolve and no kind to match.
    let bus_attr = "bus";
    // A multi-transport manifest keeps one recipe per bus kind and the spec's
    // `bus=` picks one; a single-transport manifest has already picked.
    let multi = !element_recipe.transports.is_empty();
    // Kept outside the branch: the UART single-peer diagnostic names it.
    let mut bus_name = "";
    let (bus, kind): (Option<BusId>, &str) = if multi {
        match attrs::opt_reference(node, bus_attr)
            .map_err(|issue| invalid_element(cx, node, tag, element, issue))?
        {
            Some(reference) => {
                bus_name = reference.name;
                let Some(found) = cx.bus_by_name(bus_name) else {
                    return Err(cx.report(
                        codes::DEVICE_UNKNOWN_COMPONENT,
                        span_of(node, bus_attr),
                        "unknown bus",
                        format!(
                            "<{tag} id=\"{}\"> refers to `{bus_name}`, which is not a bus in `# \
                             peripherals`",
                            id.raw
                        ),
                    ));
                };
                let kind = match cx.bus(found) {
                    Some(Bus::I2c { .. }) => "i2c",
                    Some(Bus::Spi { .. }) => "spi",
                    Some(Bus::Uart { .. }) => "uart",
                    None => {
                        return Err(cx.report(
                            codes::DEVICE_INVALID,
                            span_of(node, bus_attr),
                            "unknown bus",
                            format!(
                                "<{tag} id=\"{}\"> binds to `{bus_name}`, which is not a bus",
                                id.raw
                            ),
                        ));
                    }
                };
                (Some(found), kind)
            }
            // No reference: the manifest's own busless variant, if it has one.
            None => (None, "none"),
        }
    } else if element_recipe.bus == "none" {
        (None, "none")
    } else {
        let reference = attrs::reference(node, bus_attr)
            .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
        bus_name = reference.name;
        let Some(found) = cx.bus_by_name(bus_name) else {
            return Err(cx.report(
                codes::DEVICE_UNKNOWN_COMPONENT,
                span_of(node, bus_attr),
                "unknown bus",
                format!(
                    "<{tag} id=\"{}\"> refers to `{bus_name}`, which is not an <{}> in `# peripherals`",
                    id.raw, element_recipe.bus
                ),
            ));
        };
        let kind = match cx.bus(found) {
            Some(Bus::I2c { .. }) => "i2c",
            Some(Bus::Spi { .. }) => "spi",
            Some(Bus::Uart { .. }) => "uart",
            None => {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, bus_attr),
                    "unknown bus",
                    format!(
                        "<{tag} id=\"{}\"> binds to `{bus_name}`, which is not a bus",
                        id.raw
                    ),
                ));
            }
        };
        if kind != element_recipe.bus {
            return Err(cx.report(
                codes::DEVICE_INVALID,
                span_of(node, bus_attr),
                format!(
                    "{} needs a {} bus",
                    element_recipe.kind, element_recipe.bus
                ),
                format!(
                    "<{tag} id=\"{}\"> binds to `{bus_name}`, which is an <{kind}>, but this \
                         {} drives an <{}>",
                    id.raw, element_recipe.kind, element_recipe.bus
                ),
            ));
        }
        (Some(found), kind)
    };

    // The transport the spec selected. For a single-transport driver the
    // manifest is the selection and the bus above already matched its kind.
    let recipe: &'s DeviceRecipe = if multi {
        let Some(selected) = element_recipe
            .transports
            .iter()
            .find(|transport| transport.bus == kind)
        else {
            let supported: Vec<&str> = element_recipe
                .transports
                .iter()
                .map(|transport| transport.bus.as_str())
                .collect();
            let (summary, label) = if kind == "none" {
                (
                    format!("{} needs a bus", element_recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> declares no `bus=`, and this {} has no busless \
                         variant; it supports {}",
                        id.raw,
                        element_recipe.kind,
                        supported.join(", ")
                    ),
                )
            } else {
                (
                    format!("{} does not support this bus", element_recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> binds to `{bus_name}`, which is an <{kind}>, but this \
                         {} supports {}",
                        id.raw,
                        element_recipe.kind,
                        supported.join(", ")
                    ),
                )
            };
            return Err(cx.report(
                codes::DEVICE_INVALID,
                span_of(node, bus_attr),
                summary,
                label,
            ));
        };
        // An attribute belongs to the transport it configures: `address=` on
        // the SPI build of a dual-bus part is a typo, not a default.
        let union: Vec<&str> = element_recipe
            .transports
            .iter()
            .flat_map(transport_attrs)
            .collect();
        let allowed = transport_attrs(selected);
        if let Some(name) = union.into_iter().find(|name| {
            !allowed.contains(name)
                && node
                    .attributes
                    .iter()
                    .any(|attribute| attribute.key == *name)
        }) {
            return Err(cx.report(
                codes::DEVICE_INVALID,
                crate::attrs::key_span_of(node, name),
                format!("`{name}` is not part of the {} transport", selected.bus),
                format!(
                    "<{tag} id=\"{}\"> builds the {} transport, which takes no `{name}`",
                    id.raw, selected.bus
                ),
            ));
        }
        selected
    } else {
        element_recipe
    };
    let busless = recipe.bus == "none";

    // 5. the address an I2C part declares, or the chip select an SPI part
    // owns. Both live on the device now, not on a separate handle. A
    // composed part takes the address its `compose` block fixes.
    let literal_address = bindings.hex("address");
    let address = match recipe.address {
        // A busless driver has no address to route or check.
        _ if busless => None,
        AddressNeed::None => None,
        AddressNeed::Any | AddressNeed::Fixed(_) => {
            let read = match literal_address {
                Some(_) => None,
                None => attrs::opt_hex(node, "address")
                    .map_err(|issue| invalid_element(cx, node, tag, element, issue))?,
            };
            let address_span = match &read {
                Some(value) => value.span.start..value.span.end,
                None => span_of(node, "address"),
            };
            let value = match (literal_address, read.map(|value| value.value)) {
                (Some(literal), _) => Some(literal),
                (None, read) => read,
            };
            let got = match value {
                Some(value) => value,
                None => match recipe.address_default {
                    Some(default) => default,
                    None => {
                        return Err(cx.report(
                                codes::DEVICE_INVALID,
                                span_of(node, "address"),
                                format!("missing {} address", recipe.kind),
                                format!(
                                    "<{tag} id=\"{}\"> needs `address=\"0x..\"`; the manifest declares no default",
                                    id.raw
                                ),
                            ));
                    }
                },
            };
            if let Some((low, high)) = recipe.address_allowed {
                if !(low..=high).contains(&got) {
                    return Err(cx.report(
                            codes::DEVICE_INVALID,
                            span_of(node, "address"),
                            format!("invalid {} address", recipe.kind),
                            format!(
                                "<{tag} id=\"{}\"> address 0x{got:02X} is outside the driver's allowed range 0x{low:02X}..=0x{high:02X}",
                                id.raw
                            ),
                        ));
                }
            }
            if !espforge_schema::USABLE_I2C_ADDRESSES.contains(&got) {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    address_span.clone(),
                    format!("invalid {} address", recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> address=\"0x{got:02X}\" is reserved by the I2C \
                             specification; usable addresses are 0x08..=0x77",
                        id.raw
                    ),
                ));
            }
            match recipe.address {
                AddressNeed::Fixed(want) if got != want => {
                    return Err(cx.report(
                        codes::DEVICE_INVALID,
                        address_span.clone(),
                        format!("{} address mismatch", recipe.kind),
                        format!(
                            "<{tag} id=\"{}\"> is fixed at address 0x{want:02X}, but the \
                                 spec declares 0x{got:02X}",
                            id.raw
                        ),
                    ));
                }
                _ => {}
            }
            Some(got)
        }
    };

    let cs = if !busless && kind == "spi" && element.attrs.iter().any(|attr| attr.name == "cs") {
        // A composed part's chip select is one of the composite's pins.
        let pin = if let Some(source) = bindings.pin("cs") {
            let reference = attrs::reference(node, source)
                .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
            let Some(gpio) = cx.gpio(reference.name).cloned() else {
                return Err(unknown_gpio(cx, node, tag, id, source, reference.name));
            };
            check_cs_direction(cx, node, tag, id, source, &gpio)?;
            gpio.pin
        } else {
            // The attribute is optional on the element (another transport may
            // take no `cs`), so a missing one is reported here rather than by
            // the schema reader.
            let Some(value) = attrs::opt_pin_or_ref(node, "cs")
                .map_err(|issue| invalid_element(cx, node, tag, element, issue))?
            else {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, "cs"),
                    format!("{} has no chip select", recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> binds to an SPI bus but declares no `cs`",
                        id.raw
                    ),
                ));
            };
            match value {
                attrs::PinOrRef::Pin(pin) => pin.value,
                attrs::PinOrRef::Ref(reference) => {
                    let Some(gpio) = cx.gpio(reference.name).cloned() else {
                        return Err(unknown_gpio(cx, node, tag, id, "cs", reference.name));
                    };
                    check_cs_direction(cx, node, tag, id, "cs", &gpio)?;
                    gpio.pin
                }
            }
        };
        let claim = cx
            .claim_pin(
                Pin(pin),
                Owner::new(OwnerKind::Device(tag.to_string()), id.clone()),
                span_of(node, "cs"),
                PinUse::Unspecified,
            )
            .map_err(|refusal| refusal.report(cx))?;
        Some(claim)
    } else {
        None
    };

    // 5b. when the device is brought up, and the transfer configuration it
    // owns. `init` and `mode`/`frequency` are consumed here, like `address`
    // and `cs`, so they never become constructor slots.
    let init = if bindings.spelling(node, element, "init") == "deferred" {
        Init::Deferred
    } else {
        Init::Boot
    };
    if init == Init::Boot && recipe.runtimes.async_only() {
        return Err(cx.report(
            codes::DEVICE_INVALID,
            span_of(node, "init"),
            format!("async {} needs deferred init", recipe.kind),
            format!(
                "<{tag} id=\"{}\"> drives an async bus, which has no synchronous place to \
                     initialise before boot; write init=\"deferred\"",
                id.raw
            ),
        ));
    }
    let spi_config = if !busless && recipe.spi_config {
        let mode = bindings
            .spelling(node, element, "mode")
            .parse::<u8>()
            .unwrap_or(0);
        let frequency = bindings.spelling(node, element, "frequency");
        let khz = crate::peripherals::parse_khz(&frequency, tag, &id.raw).map_err(|label| {
            cx.report(
                codes::DEVICE_INVALID,
                span_of(node, "frequency"),
                "invalid device frequency",
                label,
            )
        })?;
        Some(DeviceSpiConfig { khz, mode })
    } else {
        None
    };
    let cs_polarity = match recipe.cs_polarity {
        espforge_schema::CsPolarity::Low => CsPolarity::ActiveLow,
        espforge_schema::CsPolarity::High => CsPolarity::ActiveHigh,
    };

    // 6. the auxiliary control lines, claimed in recipe order.
    let mut pins: Vec<(String, ClaimedPin)> = Vec::new();
    for arg in recipe.args.iter().filter_map(CtorArg::slot) {
        if arg.pin.is_none() {
            continue;
        }
        // An absent optional pin is not claimed; a required one was
        // already refused by `validate`. A part's pin is the composite's
        // (`from=`), and a part pin it does not map is absent.
        let Some(source) = bindings.source(&arg.name) else {
            continue;
        };
        // An absent optional pin is not an error; anything else the reader
        // refuses (a malformed value) must be reported rather than swallowed.
        let Some(value) = attrs::opt_pin_or_ref(node, source)
            .map_err(|issue| invalid_element(cx, node, tag, element, issue))?
        else {
            continue;
        };
        // `clk="6"` claims that GPIO directly; `clk="$gpio6"` is also checked
        // against the direction the referenced `<gpio>` declared.
        let (pin, declared) = match value {
            attrs::PinOrRef::Pin(pin) => (pin.value, None),
            attrs::PinOrRef::Ref(reference) => {
                let Some(gpio) = cx.gpio(reference.name).cloned() else {
                    return Err(unknown_gpio(cx, node, tag, id, source, reference.name));
                };
                (gpio.pin, Some((gpio.id.raw.clone(), gpio.direction)))
            }
        };
        let wanted = direction(arg.direction);
        if let Some((name, declared)) = declared
            && let Some(found) = declared.filter(|found| *found != wanted)
        {
            return Err(wrong_direction(
                cx,
                node,
                tag,
                id,
                &arg.name,
                &name,
                wanted,
                found,
            ));
        }
        let need = match wanted {
            Direction::Input => PinUse::Input,
            Direction::Output => PinUse::Output,
            Direction::Bidirectional => PinUse::Bidirectional,
        };
        let claim = cx
            .claim_pin(
                Pin(pin),
                Owner::new(OwnerKind::Device(tag.to_string()), id.clone()),
                node_span(node),
                need,
            )
            .map_err(|refusal| refusal.report(cx))?;
        pins.push((arg.name.clone(), claim));
    }

    // 7. the field type templates, parsed once, and the slots the
    // constructor and the type take.
    let ty_template = type_template(cx, node, tag, &id, &recipe.ty)?;
    let async_template = match recipe.async_mode.ty.as_deref() {
        Some(text) => Some(type_template(cx, node, tag, &id, text)?),
        None => None,
    };
    let slots = slots(
        cx,
        node,
        tag,
        id,
        element,
        recipe,
        address,
        &pins,
        &ty_template,
        async_template.as_ref(),
        bindings,
    )?;
    let factory_args = slots.factory_args;
    let args = slots.args;
    let steps = slots.steps;
    let async_steps = slots.async_steps;

    // 8. the field type, with its command slots filled.
    let ty = ty_template.fill(&slots.types);
    let async_ty = async_template
        .as_ref()
        .map(|template| template.fill(&slots.types));
    let new = rust_path(cx, node, tag, &id, "the constructor", &recipe.new)?;
    let async_new = match recipe.async_mode.new.as_deref() {
        Some(text) => Some(rust_path(
            cx,
            node,
            tag,
            &id,
            "the async constructor",
            text,
        )?),
        None => None,
    };
    let endpoint = optional_type(
        cx,
        node,
        tag,
        &id,
        "the endpoint",
        recipe.endpoint.as_deref(),
    )?;
    let async_endpoint = optional_type(
        cx,
        node,
        tag,
        &id,
        "the async endpoint",
        recipe.async_mode.endpoint.as_deref(),
    )?;

    // Where the endpoint goes among the constructor's values: a plain bus
    // argument keeps its position, a wrapper carries it inside its own
    // expression, and a busless driver has none.
    let bus_index = recipe
        .args
        .iter()
        .position(|arg| matches!(arg, CtorArg::Bus));

    // 9. the driver record: everything that is independent of the bus
    // kind, in one place.
    let driver = Driver {
        feature: recipe.feature.clone(),
        deps: recipe.deps.clone(),
        ty,
        new,
        fallible: recipe.fallible,
        pins,
        args,
        bus_index,
        factory_args,
        steps,
        async_steps,
        runtimes: crate::model::RuntimeSet {
            blocking: recipe.runtimes.blocking,
            asynchronous: recipe.runtimes.asynchronous,
        },
        init,
        async_ty,
        async_new,
        async_awaits: recipe.async_mode.awaits,
        async_feature: recipe.async_mode.feature.clone(),
        package: element.package.clone(),
        expose: element.expose.clone(),
        dependencies: if recipe.dependencies.is_empty() {
            element.dependencies.clone()
        } else {
            recipe.dependencies.clone()
        },
        manifest: element.source.clone(),
    };

    // 10. a busless driver is its pins: it leaves here with no bus id at all,
    // so `Board::bus_mode` is never asked about a bus that does not exist.
    if busless {
        if recipe.runtimes.async_only() {
            return Err(cx.report(
                codes::DEVICE_INVALID,
                span_of(node, "id"),
                "async driver without a bus",
                format!(
                    "<{tag} id=\"{}\"> is an async driver with no bus; a GPIO-timed driver \
                     cannot await a bus it does not have",
                    id.raw
                ),
            ));
        }
        return Ok(Device::Pins(PinsDevice {
            id: id.clone(),
            tag: tag.to_string(),
            driver,
        }));
    }
    let Some(bus) = bus else {
        unreachable!("a bus device resolved its bus above")
    };

    // 10b. one concrete bus kind, so emission never has to rediscover the bus
    // from independent optional fields.
    let transport = match kind {
        "i2c" => {
            let Some(address) = address else {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, "address"),
                    format!("{} has no I2C address", recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> binds to an I2C bus but declares no address",
                        id.raw
                    ),
                ));
            };
            BusTransport::I2c(I2cTransport {
                bus,
                address,
                endpoint: endpoint.clone(),
                async_endpoint: async_endpoint.clone(),
            })
        }
        "spi" => {
            let Some(cs) = cs else {
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, "cs"),
                    format!("{} has no chip select", recipe.kind),
                    format!(
                        "<{tag} id=\"{}\"> binds to an SPI bus but declares no `cs`",
                        id.raw
                    ),
                ));
            };
            BusTransport::Spi(SpiTransport {
                bus,
                cs,
                spi_config,
                cs_polarity,
                endpoint: endpoint.clone(),
                async_endpoint: async_endpoint.clone(),
            })
        }
        // A UART port has one peer: the device owns it. A second device on
        // the same port would build the peripheral twice, so it is refused
        // here instead of reaching emission.
        "uart" => {
            if let Some(previous) = cx
                .devices()
                .iter()
                .filter_map(Device::as_bus)
                .find(|device| device.bus() == bus)
            {
                return Err(cx.report(
                        codes::DEVICE_INVALID,
                        span_of(node, "bus"),
                        "the uart port is already driven",
                        format!(
                            "<{tag} id=\"{}\"> binds to `{bus_name}`, which <{} id=\"{}\"> already drives",
                            id.raw, previous.tag, previous.id.raw
                        ),
                    ));
            }
            // `espforge-runtime` compiles one `UartDevice` flavour per
            // build, so a UART driver must support the project's runtime.
            let fits = match cx.runtime() {
                Runtime::Blocking => recipe.runtimes.blocking,
                Runtime::Embassy => recipe.runtimes.asynchronous,
            };
            if !fits {
                let message = if recipe.runtimes.asynchronous {
                    format!(
                        "<{tag} id=\"{}\"> is an async driver, but this project runs on a \
                             blocking runtime; espforge-runtime compiles one UART flavour per build",
                        id.raw
                    )
                } else {
                    format!(
                        "<{tag} id=\"{}\"> is a blocking driver, but this project runs on an \
                             embassy runtime; espforge-runtime compiles one UART flavour per build",
                        id.raw
                    )
                };
                return Err(cx.report(
                    codes::DEVICE_INVALID,
                    span_of(node, "bus"),
                    "the uart driver does not match the project runtime",
                    message,
                ));
            }
            BusTransport::Uart(UartTransport {
                bus,
                endpoint: endpoint.clone(),
                async_endpoint: async_endpoint.clone(),
            })
        }
        _ => unreachable!("the bus kind was checked against the recipe"),
    };

    Ok(Device::Bus(BusDevice {
        id: id.clone(),
        tag: tag.to_string(),
        driver,
        transport,
    }))
}

/// The attributes one transport takes from the spec: the bus-level values the
/// manifest synthesizes for that kind, and nothing else.
///
/// Lowering refuses an attribute that belongs to a transport the spec did not
/// select (`address=` on an SPI build); this is the same list the manifest
/// parser wrote into the element as each transport's bus attributes.
fn transport_attrs(recipe: &DeviceRecipe) -> Vec<&'static str> {
    match recipe.bus.as_str() {
        "i2c" if !matches!(recipe.address, AddressNeed::None)
            || recipe.address_allowed.is_some() =>
        {
            vec!["address"]
        }
        "spi" if recipe.spi_config => vec!["cs", "mode", "frequency"],
        "spi" => vec!["cs"],
        _ => Vec::new(),
    }
}

/// Where a composed part's attributes come from.
///
/// The composite writes the part's pins and literals on its own element, so the
/// lowerer reads a name here before it reads the node. The composite itself has
/// an empty `Bindings` and reads the node exactly as it always did.
struct Bindings<'a> {
    /// Whether this is a composed part rather than the element the spec writes.
    part: bool,
    /// The composite attribute each part pin reads (`cs` -> `touch_cs`).
    pins: BTreeMap<&'a str, &'a str>,
    /// The literal each part attribute takes (`address` -> `0x48`).
    values: BTreeMap<&'a str, &'a str>,
}

impl<'a> Bindings<'a> {
    /// The element the spec wrote.
    fn none() -> Self {
        Self {
            part: false,
            pins: BTreeMap::new(),
            values: BTreeMap::new(),
        }
    }

    /// One `compose` block.
    fn of(part: &'a RecipePart) -> Self {
        let mut bindings = Bindings::none();
        bindings.part = true;
        for attr in &part.attrs {
            match &attr.source {
                PartSource::From(name) => {
                    bindings.pins.insert(&attr.name, name);
                }
                PartSource::Value(value) => {
                    bindings.values.insert(&attr.name, value);
                }
            }
        }
        bindings
    }

    /// The name on the node this pin reads, or `None` when a part does not map
    /// it.
    fn source<'b>(&'b self, name: &'b str) -> Option<&'b str> {
        if !self.part {
            return Some(name);
        }
        self.pins.get(name).copied()
    }

    /// The composite pin this part attribute reads, if any.
    fn pin(&self, name: &str) -> Option<&'a str> {
        self.pins.get(name).copied()
    }

    /// The value written for `name`: the fixed literal, else the node, else the
    /// element's default.
    fn spelling(
        &self,
        node: &ParsedNode<'_>,
        element: &espforge_schema::Element,
        name: &str,
    ) -> String {
        match self.values.get(name) {
            Some(value) => (*value).to_string(),
            None => spelling(node, element, name).to_string(),
        }
    }

    /// A fixed address (`value="0x48"`).
    fn hex(&self, name: &str) -> Option<u8> {
        self.values.get(name).and_then(|text| hex_value(text))
    }
}

/// A hexadecimal byte, as a `compose` block writes an address.
fn hex_value(text: &str) -> Option<u8> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u8::from_str_radix(digits, 16).ok()
}

/// The slots one recipe's arguments fill, in recipe order.
struct Slots {
    /// The constructor arguments.
    args: Vec<RustExpr>,
    /// The same arguments, split for a deferred factory.
    factory_args: Vec<FactoryArg>,
    /// The `then` steps of the blocking constructor.
    steps: Vec<Step>,
    /// The `then` steps of the async constructor.
    async_steps: Vec<Step>,
    /// The values a type template takes, by slot name.
    types: BTreeMap<String, RustType>,
}

/// Resolve every declared argument to the Rust expression it contributes.
#[allow(clippy::too_many_arguments)]
fn slots(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    element: &espforge_schema::Element,
    recipe: &DeviceRecipe,
    address: Option<u8>,
    pins: &[(String, ClaimedPin)],
    ty: &TypeTemplate,
    async_ty: Option<&TypeTemplate>,
    bindings: &Bindings<'_>,
) -> Result<Slots, Reported> {
    let mut args = Vec::new();
    let mut factory_args = Vec::new();
    let mut types = BTreeMap::new();
    // What a `then` step may name: the constructor's own values, in the
    // expression the call gets at boot and the one a factory rebuilds after
    // boot. Anything else is a manifest error the schema already reported.
    let mut boot_values: BTreeMap<String, RustExpr> = BTreeMap::new();
    let mut factory_values: BTreeMap<String, RustExpr> = BTreeMap::new();
    // `{address}` is the element's. A recipe that maps it turns each address
    // into the crate's own value; without a map it is the 7-bit number.
    let mapped_address = address.and_then(|address| {
        recipe.address_map.as_ref().map(|entries| {
            entries
                .iter()
                .find(|(known, _)| *known == address)
                .map(|(_, text)| text.as_str())
                .unwrap_or_else(|| panic!("`<{tag}>` has no mapping for address 0x{address:02X}"))
        })
    });
    for arg in &recipe.args {
        // The endpoint does not exist until the bus is emitted: the model
        // inserts it at this argument's position, so lowering skips it.
        if matches!(arg, CtorArg::Bus) {
            continue;
        }
        // The address travels to a deferred factory as a constant.
        if matches!(arg, CtorArg::Address) {
            let Some(address) = address else {
                // The schema refuses `{address}` on a bus that has none.
                continue;
            };
            let expression = match mapped_address {
                Some(text) => rust_expr(cx, node, tag, id, "the mapped address", text)?,
                None => RustExpr::number(address as u64),
            };
            args.push(expression.clone());
            factory_args.push(FactoryArg::Constant(expression.clone()));
            boot_values.insert("address".to_string(), expression.clone());
            factory_values.insert("address".to_string(), expression);
            continue;
        }
        // A fixed argument is the manifest's own expression; it rides the same
        // path, so boot, deferred and async agree on its position.
        //
        // `{address}` is the element's, and the element is the only thing that
        // knows it. A `{bus_device}` hole stays a sentinel: the endpoint does
        // not exist until emission builds the bus.
        if let CtorArg::Fixed { text, .. } = arg {
            // Parse the template once: `{bus_device}` stays a sentinel for
            // emission, and `{address}` is spliced here as an expression
            // rather than text, so a string literal keeps its braces.
            let template = RustExpr::parse_template(text).map_err(|message| {
                invalid_recipe(cx, node, tag, id, "a constructor argument", &message)
            })?;
            let expression = match address {
                Some(address) => {
                    let value = match mapped_address {
                        Some(text) => rust_expr(cx, node, tag, id, "the mapped address", text)?,
                        None => RustExpr::number(address as u64),
                    };
                    template.splice(&BTreeMap::from([("address".to_string(), value)]))
                }
                None => template,
            };
            args.push(expression.clone());
            factory_args.push(FactoryArg::Constant(expression));
            continue;
        }
        let Some(arg) = arg.slot() else {
            continue;
        };
        match &arg.pin {
            // A pin slot is the claimed line, wrapped in `Some(..)` when the
            // attribute is optional: the driver's argument is an `Option`.
            Some(template) => {
                let claimed = pins.iter().find(|(name, _)| *name == arg.name);
                let optional = !required(element, &arg.name);
                let expression = match claimed {
                    Some((_, pin)) => {
                        let text = template.replace("{n}", &pin.pin().0.to_string());
                        let expression = rust_expr(
                            cx,
                            node,
                            tag,
                            id,
                            &format!("the pin `{}`", arg.name),
                            &text,
                        )?;
                        if optional {
                            RustExpr::some(&expression)
                        } else {
                            expression
                        }
                    }
                    None => RustExpr::none(),
                };
                args.push(expression.clone());
                // A step may name the line as `{dc}` or `{dc_pin}`, and a
                // deferred factory rebuilds it from the stored peripheral.
                let rebuilt = match claimed {
                    Some(_) => {
                        let field = quote::format_ident!("pin_{}", arg.name);
                        let build = crate::model::line_from(
                            direction(arg.direction),
                            arg.level,
                            arg.pull,
                            arg.open_drain,
                            quote::quote! { self.#field },
                        );
                        let build = if optional {
                            quote::quote! { self.#field.map(|pin| #build) }
                        } else {
                            build
                        };
                        rust_expr(
                            cx,
                            node,
                            tag,
                            id,
                            &format!("the pin `{}`", arg.name),
                            &build.to_string(),
                        )?
                    }
                    None => RustExpr::none(),
                };
                for name in [arg.name.clone(), format!("{}_pin", arg.name)] {
                    boot_values.insert(name.clone(), expression.clone());
                    factory_values.insert(name, rebuilt.clone());
                }
                factory_args.push(match claimed {
                    Some(_) => FactoryArg::Pin {
                        name: arg.name.clone(),
                        optional,
                        direction: direction(arg.direction),
                        level: arg.level,
                        pull: arg.pull,
                        open_drain: arg.open_drain,
                    },
                    None => FactoryArg::Constant(RustExpr::none()),
                });
            }
            // A mapped slot is the value its spelling carries.
            None => {
                let needs_type =
                    ty.uses(&arg.name) || async_ty.is_some_and(|template| template.uses(&arg.name));
                let value = mapped(cx, node, tag, id, element, arg, needs_type, bindings)?;
                if let Some(value_ty) = &value.ty {
                    types.insert(arg.name.clone(), value_ty.clone());
                }
                let expression = value.expr.unwrap_or_else(RustExpr::none);
                args.push(expression.clone());
                factory_args.push(FactoryArg::Constant(expression.clone()));
                boot_values.insert(arg.name.clone(), expression.clone());
                factory_values.insert(arg.name.clone(), expression);
            }
        }
    }
    let steps = resolve_steps(cx, node, tag, id, &recipe.steps, &boot_values, &factory_values)?;
    let async_steps = resolve_steps(
        cx,
        node,
        tag,
        id,
        &recipe.async_mode.steps,
        &boot_values,
        &factory_values,
    )?;
    Ok(Slots {
        args,
        factory_args,
        steps,
        async_steps,
        types,
    })
}

/// One runtime's `then` steps, with their argument holes filled.
///
/// The receiver stays a placeholder: emission is the only place that knows the
/// local the device was built into.
fn resolve_steps(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    steps: &[espforge_schema::Step],
    boot: &BTreeMap<String, RustExpr>,
    factory: &BTreeMap<String, RustExpr>,
) -> Result<Vec<Step>, Reported> {
    steps
        .iter()
        .map(|step| {
            let call = step.call().map_err(|message| {
                invalid_recipe(cx, node, tag, id, "a `then` step", &message)
            })?;
            Ok(Step {
                boot: call.splice(boot),
                factory: call.splice(factory),
                awaits: step.awaits,
                rebind: step.rebind,
                fallible: step.fallible,
            })
        })
        .collect()
}

/// The value one mapped `arg` resolves to.
#[allow(clippy::too_many_arguments)]
fn mapped(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    element: &espforge_schema::Element,
    arg: &DeviceArg,
    needs_type: bool,
    bindings: &Bindings<'_>,
) -> Result<MappedValue, Reported> {
    let spelling = bindings.spelling(node, element, &arg.name);
    let text = arg
        .map
        .as_ref()
        .and_then(|map| {
            map.iter()
                .find(|(value, _)| *value == spelling)
                .map(|(_, expression)| expression.as_str())
        })
        .unwrap_or_else(|| {
            panic!(
                "`<{}>` has no mapping for {}=\"{spelling}\"",
                element.tag, arg.name
            )
        });
    MappedValue::parse(text, needs_type, true).map_err(|message| {
        invalid_recipe(
            cx,
            node,
            tag,
            id,
            &format!("the value of `{}`", arg.name),
            &message,
        )
    })
}

/// A device recipe's field type template, parsed once.
fn type_template(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    text: &str,
) -> Result<TypeTemplate, Reported> {
    TypeTemplate::parse(text)
        .map_err(|message| invalid_recipe(cx, node, tag, id, "the field type", &message))
}

/// A concrete Rust type from the schema (an endpoint).
fn rust_type(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: &str,
) -> Result<RustType, Reported> {
    RustType::parse(text)
        .map_err(|error| invalid_recipe(cx, node, tag, id, what, &error.to_string()))
}

/// An optional concrete Rust type from the schema (an endpoint).
fn optional_type(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: Option<&str>,
) -> Result<Option<RustType>, Reported> {
    match text {
        Some(text) => rust_type(cx, node, tag, id, what, text).map(Some),
        None => Ok(None),
    }
}

/// A Rust path from the schema (a constructor).
fn rust_path(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: &str,
) -> Result<RustPath, Reported> {
    RustPath::parse(text)
        .map_err(|error| invalid_recipe(cx, node, tag, id, what, &error.to_string()))
}

/// A Rust expression from the schema (a filled pin template).
fn rust_expr(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: &str,
) -> Result<RustExpr, Reported> {
    RustExpr::parse(text)
        .map_err(|error| invalid_recipe(cx, node, tag, id, what, &error.to_string()))
}

/// The located diagnostic for a recipe the schema accepted but could not be
/// expressed as Rust. The schema validates these, so this is a safety net, not
/// a user-facing path.
fn invalid_recipe(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    message: &str,
) -> Reported {
    cx.report(
        codes::DEVICE_INVALID,
        node_span(node),
        format!("invalid {tag} recipe"),
        format!(
            "<{tag} id=\"{}\">: {what} is not Rust syntax: {message}",
            id.raw
        ),
    )
}

/// A chip select is an output line. A `cs="$input_gpio"` would drive a pin
/// declared as an input, so refuse it before the claim.
fn check_cs_direction(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    attr: &str,
    gpio: &crate::model::GpioDecl,
) -> Result<(), Reported> {
    if let Some(found) = gpio.direction.filter(|found| *found != Direction::Output) {
        return Err(wrong_direction(
            cx,
            node,
            tag,
            id,
            attr,
            &gpio.id.raw,
            Direction::Output,
            found,
        ));
    }
    Ok(())
}

/// The board model's direction for one the schema declared.
fn direction(value: espforge_schema::PinDirection) -> Direction {
    match value {
        espforge_schema::PinDirection::Input => Direction::Input,
        espforge_schema::PinDirection::Output => Direction::Output,
        espforge_schema::PinDirection::Bidirectional => Direction::Bidirectional,
        // An open-drain line is one the driver both drives and reads; the
        // wrapper it is built into is chosen at emission from `open_drain`.
        espforge_schema::PinDirection::OpenDrain => Direction::Bidirectional,
    }
}

/// Whether the schema marks `name` required on `element`.
fn required(element: &espforge_schema::Element, name: &str) -> bool {
    element
        .attrs
        .iter()
        .any(|attr| attr.name == name && attr.required)
}

/// The value written for `name`, or the default the schema gives it.
fn spelling<'a>(
    node: &'a ParsedNode<'a>,
    element: &'a espforge_schema::Element,
    name: &str,
) -> &'a str {
    node.get_attr(name)
        .or_else(|| {
            element
                .attrs
                .iter()
                .find(|attr| attr.name == name)
                .and_then(|attr| attr.default.as_deref())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_template_takes_its_slot_values() {
        let mut values = BTreeMap::new();
        values.insert(
            "size".to_string(),
            RustType::parse("espforge_drivers::Oled128x64").unwrap(),
        );
        let filled = TypeTemplate::parse("espforge_drivers::Ssd1306Display<{size}>")
            .unwrap()
            .fill(&values);
        assert_eq!(
            filled,
            RustType::parse("espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>")
                .unwrap()
        );
        // No placeholders, no change.
        let plain = TypeTemplate::parse("espforge_drivers::Ili9341Display")
            .unwrap()
            .fill(&values);
        assert_eq!(
            plain,
            RustType::parse("espforge_drivers::Ili9341Display").unwrap()
        );
    }
}

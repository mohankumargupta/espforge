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
    AddressNeed, DeviceArg, DeviceRecipe, MappedValue, PartSource, RecipePart, RustExpr, RustPath,
    RustType, TypeTemplate,
};
use std::collections::BTreeMap;

use crate::attrs;
use crate::devices::{invalid_element, invalid_id, span_of, unknown_gpio, wrong_direction};
use crate::element::{Element, LowerCx, Reported, Section, is_rust_ident};
use crate::generated;
use crate::model::{
    Bus, BusDevice, BusTransport, ClaimedPin, CsPolarity, Device, Direction, Driver, FactoryArg,
    I2cTransport, Ident, Init, Owner, OwnerKind, Pin, PinUse, Runtime,
    SpiConfig as DeviceSpiConfig, SpiTransport, UartTransport,
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
        cx.push_device(Device::Bus(device));

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
            cx.push_device(Device::Bus(device));
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
    recipe: &'s DeviceRecipe,
    id: &Ident,
    bindings: &Bindings<'s>,
) -> Result<BusDevice, Reported> {
    // 3. the bus peripheral, of the kind the recipe declares.
    //
    // Whether the bus is blocking or async is a property of the bus, not
    // of the project: a blocking display on an embassy board is legal, so
    // the scheduler is no longer consulted here.
    let bus_attr = "bus";
    let reference = attrs::reference(node, bus_attr)
        .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
    let bus_name = reference.name;
    let Some(bus) = cx.bus_by_name(bus_name) else {
        return Err(cx.report(
            codes::DEVICE_UNKNOWN_COMPONENT,
            span_of(node, bus_attr),
            "unknown bus",
            format!(
                "<{tag} id=\"{}\"> refers to `{bus_name}`, which is not an <{}> in `# peripherals`",
                id.raw, recipe.bus
            ),
        ));
    };
    let kind = match cx.bus(bus) {
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
    if kind != recipe.bus {
        return Err(cx.report(
            codes::DEVICE_INVALID,
            span_of(node, bus_attr),
            format!("{} needs a {} bus", recipe.kind, recipe.bus),
            format!(
                "<{tag} id=\"{}\"> binds to `{bus_name}`, which is an <{kind}>, but this \
                     {} drives an <{}>",
                id.raw, recipe.kind, recipe.bus
            ),
        ));
    }

    // 5. the address an I2C part declares, or the chip select an SPI part
    // owns. Both live on the device now, not on a separate handle. A
    // composed part takes the address its `compose` block fixes.
    let literal_address = bindings.hex("address");
    let address = match recipe.address {
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

    let cs = if kind == "spi" && element.attrs.iter().any(|attr| attr.name == "cs") {
        // A composed part's chip select is one of the composite's pins.
        let pin = if let Some(source) = bindings.pin("cs") {
            let reference = attrs::reference(node, source)
                .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
            let Some(gpio) = cx.gpio(reference.name).cloned() else {
                return Err(unknown_gpio(cx, node, tag, id, source, reference.name));
            };
            gpio.pin
        } else {
            let value = attrs::pin_or_ref(node, "cs")
                .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
            match value {
                attrs::PinOrRef::Pin(pin) => pin.value,
                attrs::PinOrRef::Ref(reference) => {
                    let Some(gpio) = cx.gpio(reference.name).cloned() else {
                        return Err(unknown_gpio(cx, node, tag, id, "cs", reference.name));
                    };
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
    let spi_config = if recipe.spi_config {
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
    for arg in &recipe.args {
        if arg.pin.is_none() {
            continue;
        }
        // An absent optional pin is not claimed; a required one was
        // already refused by `validate`. A part's pin is the composite's
        // (`from=`), and a part pin it does not map is absent.
        let Some(source) = bindings.source(&arg.name) else {
            continue;
        };
        let Ok(reference) = attrs::reference(node, source) else {
            continue;
        };
        let Some(gpio) = cx.gpio(reference.name).cloned() else {
            return Err(unknown_gpio(cx, node, tag, id, source, reference.name));
        };
        let wanted = direction(arg.direction);
        if let Some(found) = gpio.direction.filter(|found| *found != wanted) {
            return Err(wrong_direction(
                cx,
                node,
                tag,
                &id,
                &arg.name,
                &gpio.id.raw,
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
                Pin(gpio.pin),
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
        &pins,
        &ty_template,
        async_template.as_ref(),
        bindings,
    )?;
    let mut factory_args = slots.factory_args;
    // The address is a constructor argument like any other, so a deferred
    // factory must carry it too (it is a constant, not a pin).
    if let Some(address) = address {
        factory_args.insert(0, FactoryArg::Constant(RustExpr::number(address as u64)));
    }
    let args = slots.args;

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

    // 9. the driver record: everything that is independent of the bus
    // kind, in one place.
    let driver = Driver {
        feature: recipe.feature.clone(),
        deps: recipe.deps.clone(),
        ty,
        new,
        pins,
        args,
        factory_args,
        runtimes: crate::model::RuntimeSet {
            blocking: recipe.runtimes.blocking,
            asynchronous: recipe.runtimes.asynchronous,
        },
        init,
        async_ty,
        async_new,
        async_feature: recipe.async_mode.feature.clone(),
        package: element.package.clone(),
        expose: element.expose.clone(),
        dependencies: element.dependencies.clone(),
        manifest: element.source.clone(),
    };

    // 10. the transport: one concrete bus kind, so emission never has to
    // rediscover the bus from independent optional fields.
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

    Ok(BusDevice {
        id: id.clone(),
        tag: tag.to_string(),
        driver,
        transport,
    })
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
    pins: &[(String, ClaimedPin)],
    ty: &TypeTemplate,
    async_ty: Option<&TypeTemplate>,
    bindings: &Bindings<'_>,
) -> Result<Slots, Reported> {
    let mut args = Vec::new();
    let mut factory_args = Vec::new();
    let mut types = BTreeMap::new();
    for arg in &recipe.args {
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
                args.push(expression);
                factory_args.push(match claimed {
                    Some(_) => FactoryArg::Pin {
                        name: arg.name.clone(),
                        optional,
                        direction: direction(arg.direction),
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
                factory_args.push(FactoryArg::Constant(expression));
            }
        }
    }
    Ok(Slots {
        args,
        factory_args,
        types,
    })
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

/// The board model's direction for one the schema declared.
fn direction(value: espforge_schema::PinDirection) -> Direction {
    match value {
        espforge_schema::PinDirection::Input => Direction::Input,
        espforge_schema::PinDirection::Output => Direction::Output,
        espforge_schema::PinDirection::Bidirectional => Direction::Bidirectional,
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

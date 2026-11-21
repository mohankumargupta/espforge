//! The v2 `driver` manifest: a typed front end for the same model.
//!
//! A `driver "<tag>"` file carries no raw Rust. Its `codegen` block names a
//! type and a constructor as templates whose only holes are the manifest's own
//! `attr`/`pin` names plus `{bus_device}`, `{cs_pin}`, `{spi_device}` and
//! `{address}`. Every template is parsed with `syn` after the holes are
//! replaced by sentinel identifiers, so a typo is a manifest error at load
//! rather than a `rustc` error in a generated project.
//!
//! The parser compiles the result into the v1 [`Element`] shape: a
//! [`DeviceRecipe`] plus `AttrDef`s. Lowering and emission therefore keep one
//! interpreter, and the v1/v2 equivalence test compares recipes rather than
//! trusting two code paths to agree.

use std::collections::BTreeMap;
use std::ops::Range;

use syn::{Expr, Type};

use super::*;

/// One `transport "<kind>" { bus { .. } codegen { .. } }` block.
struct TransportDecl {
    kind: String,
    bus: BusDecl,
    codegen: Codegen,
    /// `dependency` blocks that amend the driver's crates for this transport.
    dependencies: Vec<Dependency>,
    span: Range<usize>,
}

/// One `bus "<kind>"` block.
struct BusDecl {
    kind: String,
    address: AddressNeed,
    address_default: Option<u8>,
    address_allowed: Option<(u8, u8)>,
    spi_config: bool,
    mode: Option<String>,
    frequency: Option<String>,
    cs_polarity: CsPolarity,
    span: Range<usize>,
}

/// One declarative `pin "<name>"` line.
struct PinDecl {
    name: String,
    direction: PinDirection,
    /// The output level the line starts at; unused for an input.
    level: Level,
    /// The input's internal pull resistor; unused for an output.
    pull: Pull,
    optional: bool,
    /// `via="ref"`: the pin must be a `$<gpio>` reference, not a bare number.
    /// The default (`pin_or_ref`) also accepts `clk="6"`, which is how a
    /// busless driver names the lines it owns.
    ref_only: bool,
    span: Range<usize>,
}

#[derive(Clone, Copy)]
enum Level {
    Low,
    High,
}

impl Level {
    fn word(self) -> &'static str {
        match self {
            Level::Low => "Low",
            Level::High => "High",
        }
    }
}

/// One `pull="up|down|none"` on an input pin.
#[derive(Clone, Copy, PartialEq)]
enum Pull {
    None,
    Up,
    Down,
}

impl Pull {
    fn word(self) -> &'static str {
        match self {
            Pull::None => "None",
            Pull::Up => "Up",
            Pull::Down => "Down",
        }
    }
}

/// One `runtime "blocking"` or `runtime "async"` block.
#[derive(Default)]
struct RuntimeDecl {
    ty: Option<(String, Range<usize>)>,
    constructor: Option<(String, Range<usize>)>,
    endpoint: Option<String>,
    feature: Option<String>,
    /// `awaits #false`: the constructor is a plain `fn` on the async path.
    awaits: Option<bool>,
    /// `then { call ".." }` steps, in written order.
    steps: Vec<StepDecl>,
    span: Range<usize>,
}

/// One `then` step: a call the device runs after its constructor.
///
/// `call "init({delay})"` plus its flags: `awaits #false` on the async path,
/// `rebind #true` when the call hands back the device it consumed, and
/// `fallible #false` when it returns `()` or `&mut Self`.
struct StepDecl {
    text: String,
    awaits: bool,
    rebind: bool,
    fallible: bool,
    span: Range<usize>,
}

/// The `codegen` block.
#[derive(Default)]
struct Codegen {
    feature: Option<String>,
    blocking: Option<RuntimeDecl>,
    asynchronous: Option<RuntimeDecl>,
    maps: Vec<MapDecl>,
    /// Whether the constructor returns `Result` (`fallible #false` names a
    /// crate whose `new()` returns the device directly).
    fallible: bool,
    span: Range<usize>,
}

struct MapDecl {
    name: String,
    entries: Vec<(String, MapValue)>,
    span: Range<usize>,
}

enum MapValue {
    Type(String),
    Expr(String),
}

/// Parse one top-level `driver` node into `schema`.
pub(crate) fn parse_driver(node: &KdlNode, schema: &mut Schema, errors: &mut Vec<SchemaError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "driver has no tag",
            "write `driver \"tag\" { .. }`",
        ));
        return;
    }
    let (tag, tag_span) = names.remove(0);
    if RESERVED_NAMES.contains(&tag.as_str()) {
        errors.push(err(
            tag_span,
            format!("`{tag}` is a reserved name"),
            "a driver cannot be called help, examples or overview",
        ));
        return;
    }
    if schema.element(&tag).is_some() {
        errors.push(err(
            tag_span,
            format!("duplicate tag `{tag}`"),
            "tags are unique across sections",
        ));
        return;
    }

    let mut format_version = None;
    let mut summary = None;
    let mut description = None;
    let mut category = None;
    let mut dependencies: Vec<Dependency> = Vec::new();
    let mut bus: Option<BusDecl> = None;
    let mut pins: Vec<PinDecl> = Vec::new();
    let mut declared_attrs: Vec<AttrDef> = Vec::new();
    let mut parts: Vec<RecipePart> = Vec::new();
    let mut codegen: Option<Codegen> = None;
    let mut transports: Vec<TransportDecl> = Vec::new();
    let mut examples: Vec<Example> = Vec::new();
    let mut see_also: Vec<String> = Vec::new();

    for child in children(node) {
        match child.name().value() {
            "summary" | "description" | "category" => {
                let slot = match child.name().value() {
                    "summary" => &mut summary,
                    "description" => &mut description,
                    _ => &mut category,
                };
                let value = positional_strings(child, errors).into_iter().next();
                if slot.is_some() {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` given twice", child.name().value()),
                        "keep one",
                    ));
                } else if let Some((value, _)) = value {
                    *slot = Some(value);
                } else {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` takes a string", child.name().value()),
                        "quote it",
                    ));
                }
            }
            "format_version" => {
                if format_version.is_some() {
                    errors.push(err(
                        node_range(child),
                        "`format_version` given twice",
                        "keep one",
                    ));
                    continue;
                }
                let value = child
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .and_then(|entry| {
                        entry
                            .value()
                            .as_integer()
                            .or_else(|| entry.value().as_string().and_then(|s| s.parse().ok()))
                    });
                match value {
                    Some(2) => format_version = Some(2),
                    _ => errors.push(err(
                        node_range(child),
                        "`format_version` must be 2",
                        "a v2 driver declares `format_version 2`",
                    )),
                }
            }
            "dependency" => {
                if let Some(dependency) = parse_dependency(child, &tag, false, errors) {
                    dependencies.push(dependency);
                }
            }
            "bus" => {
                if bus.is_some() {
                    errors.push(err(node_range(child), "`bus` given twice", "keep one"));
                } else {
                    bus = parse_bus(child, None, errors);
                }
            }
            "pin" => {
                if let Some(pin) = parse_pin(child, errors) {
                    if pins.iter().any(|known| known.name == pin.name) {
                        errors.push(err(
                            pin.span,
                            format!("duplicate `pin` `{}`", pin.name),
                            "each pin is declared once",
                        ));
                    } else {
                        pins.push(pin);
                    }
                }
            }
            "attr" => parse_attr(child, &mut declared_attrs, errors),
            "compose" => parse_compose(child, &mut parts, errors),
            "codegen" => {
                if codegen.is_some() {
                    errors.push(err(node_range(child), "`codegen` given twice", "keep one"));
                } else {
                    codegen = parse_codegen(child, errors);
                }
            }
            "transport" => {
                if let Some(transport) = parse_transport(child, errors) {
                    if transports
                        .iter()
                        .any(|known| known.kind == transport.kind)
                    {
                        errors.push(err(
                            transport.span,
                            format!("duplicate `transport` `{}`", transport.kind),
                            "each transport kind is declared once",
                        ));
                    } else {
                        transports.push(transport);
                    }
                }
            }
            "example" => parse_example(child, &mut examples, errors),
            "see_also" => {
                for (target, target_span) in positional_strings(child, errors) {
                    if target.split_once('/').is_none() {
                        errors.push(err(
                            target_span,
                            format!("`{target}` is not a `section/leaf` target"),
                            "write for example `peripherals/i2c`",
                        ));
                    } else {
                        see_also.push(target);
                    }
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in driver `{tag}`"),
                "expected format_version, summary, description, category, dependency, bus, pin, \
                 attr, compose, codegen, transport, example or see_also",
            )),
        }
    }

    if format_version.is_none() {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` has no `format_version`"),
            "write `format_version 2`",
        ));
    }
    if summary.is_none() {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` has no summary"),
            "write `summary \"...\"`",
        ));
    }
    if examples.is_empty() {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` has no example"),
            "every driver carries at least one `example \"title\" \"fragment\"`",
        ));
    }
    // A driver describes one transport at the top level (`bus` + `codegen`)
    // or one per bus kind with `transport "<kind>" { .. }` blocks.
    let multi = !transports.is_empty();
    if multi && (bus.is_some() || codegen.is_some()) {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` declares both `transport` and a top-level `bus`/`codegen`"),
            "a multi-transport driver keeps its bus and codegen inside each `transport` block",
        ));
        return;
    }
    if multi && transports.len() < 2 {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` declares only one `transport`"),
            "two transports make a multi-transport driver; one is `bus \"..\"` + `codegen`",
        ));
        return;
    }
    let single: Option<(&BusDecl, &Codegen)> = if multi {
        None
    } else {
        let Some(bus) = bus.as_ref() else {
            errors.push(err(
                span.clone(),
                format!("driver `{tag}` has no `bus`"),
                "write `bus \"i2c\" { .. }`, `bus \"spi\" { .. }` or `bus \"uart\"`",
            ));
            return;
        };
        let Some(codegen) = codegen.as_ref() else {
            errors.push(err(
                span.clone(),
                format!("driver `{tag}` has no `codegen`"),
                "name at least one `runtime \"blocking\"` or `runtime \"async\"` block",
            ));
            return;
        };
        Some((bus, codegen))
    };

    let mut attrs: Vec<AttrDef> = Vec::new();
    attrs.push(AttrDef {
        name: "id".into(),
        kind: AttrKind::Ident,
        required: true,
        default: None,
        range: None,
        help: None,
        span: tag_span.clone(),
    });
    match &single {
        // A busless driver consumes no peripheral, so it declares no `bus`
        // attribute at all: writing `bus="$i2c0"` on it is an unknown
        // attribute, which is the diagnostic a reader needs.
        Some((bus, _)) => {
            if bus.kind == "none" {
                if pins.is_empty() {
                    errors.push(err(
                        span.clone(),
                        format!("driver `{tag}` declares no pin"),
                        "a `bus \"none\"` driver owns the lines it claims: declare at least one \
                         `pin`",
                    ));
                    return;
                }
            } else {
                attrs.push(AttrDef {
                    name: "bus".into(),
                    kind: AttrKind::Ref {
                        to: vec![bus.kind.clone()],
                    },
                    required: true,
                    default: None,
                    range: None,
                    help: None,
                    span: bus.span.clone(),
                });
            }
            add_bus_attrs(bus, &mut attrs);
        }
        // A multi-transport element accepts the union of its transports' bus
        // attributes. Which are required is decided by the bus the spec binds
        // to, so each is optional here and lowering enforces the one it chose.
        None => {
            let kinds: Vec<String> = transports
                .iter()
                .map(|transport| transport.kind.clone())
                .filter(|kind| kind != "none")
                .collect();
            if !kinds.is_empty() {
                attrs.push(AttrDef {
                    name: "bus".into(),
                    kind: AttrKind::Ref { to: kinds },
                    // Omitting the bus selects the manifest's own busless
                    // variant, when it has one.
                    required: !transports
                        .iter()
                        .any(|transport| transport.kind == "none"),
                    default: None,
                    range: None,
                    help: None,
                    span: span.clone(),
                });
            }
            for transport in &transports {
                add_union_bus_attrs(&transport.bus, &mut attrs);
            }
        }
    }
    for pin in &pins {
        // `pin_or_ref` by default: a busless driver names its lines as bare
        // numbers (`clk="6"`), and a bus device keeps `$<gpio>` working too.
        // `via="ref"` restores the strict reference and its direction check.
        let kind = if pin.ref_only {
            AttrKind::Ref {
                to: vec!["gpio".into()],
            }
        } else {
            AttrKind::PinOrRef
        };
        attrs.push(AttrDef {
            name: pin.name.clone(),
            kind,
            required: !pin.optional,
            default: None,
            range: None,
            help: None,
            span: pin.span.clone(),
        });
    }
    for attr in declared_attrs {
        if attrs.iter().any(|known| known.name == attr.name) {
            errors.push(err(
                attr.span.clone(),
                format!("duplicate attribute `{}`", attr.name),
                "the bus, a pin and an `attr` cannot share a name",
            ));
        } else {
            attrs.push(attr);
        }
    }

    // Build one recipe per transport. Every transport shares the driver's
    // pins, attributes and `compose` blocks; only its bus and codegen differ.
    let (recipe, transport_recipes) = if let Some((bus, codegen)) = single {
        let Some(recipe) = build_recipe(
            &tag,
            bus,
            &pins,
            &attrs,
            codegen,
            &dependencies,
            &parts,
            errors,
        ) else {
            return;
        };
        (recipe, Vec::new())
    } else {
        let mut built: Vec<DeviceRecipe> = Vec::new();
        for transport in &transports {
            // A transport may amend the driver's dependency set: a dual-bus
            // crate enables only the selected transport's Cargo feature.
            let mut merged = dependencies.clone();
            for dependency in &transport.dependencies {
                absorb_dependency(&mut merged, dependency, &tag, transport, errors);
            }
            let Some(mut recipe) = build_recipe(
                &tag,
                &transport.bus,
                &pins,
                &attrs,
                &transport.codegen,
                &merged,
                &parts,
                errors,
            ) else {
                return;
            };
            recipe.dependencies = merged;
            built.push(recipe);
        }
        // The element's own list is the union: it answers "what does this
        // driver name at all" for readers that do not pick a transport.
        // Emission reads each recipe's own list, so a project never enables
        // the features of a transport it did not use.
        for dependency in built
            .iter()
            .flat_map(|recipe| recipe.dependencies.iter())
        {
            match dependencies
                .iter_mut()
                .find(|known| known.package == dependency.package)
            {
                Some(known) if known.same_source(dependency) => known.absorb(dependency),
                Some(_) => {}
                None => dependencies.push(dependency.clone()),
            }
        }
        let first = built
            .first()
            .cloned()
            .expect("a multi-transport driver declares at least two transports");
        (first, built)
    };

    let kind = category.clone().unwrap_or_else(|| "device".to_string());
    let mut recipe = recipe;
    recipe.kind = kind.clone();
    recipe.transports = transport_recipes;
    for transport in &mut recipe.transports {
        transport.kind = kind.clone();
    }

    // v1 compatibility fields: `package` is the implementation crate, `expose`
    // the auxiliary ones. The emitter reads `dependencies` first; the pair is
    // kept so path resolution and lock v1 readers see the same shape.
    let package = if recipe.feature.is_empty() {
        implementation_dependency(&recipe, &dependencies)
    } else {
        None
    };
    let expose: Vec<Expose> = dependencies
        .iter()
        .filter(|dependency| {
            package
                .as_ref()
                .is_none_or(|package| package.package != dependency.package)
        })
        .map(|dependency| Expose {
            package: dependency.package.clone(),
            version: dependency.version.clone(),
        })
        .collect();

    schema.elements.push(Element {
        tag,
        section: "devices".into(),
        summary: summary.unwrap_or_default(),
        description,
        attrs,
        examples,
        see_also,
        provides: None,
        recipe: None,
        device_recipe: Some(recipe),
        service_recipe: None,
        package,
        expose,
        dependencies,
        format: Format::V2,
        source: None,
        span,
    });
}

/// Parse one `transport` block of a multi-transport `driver`.
fn parse_transport(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<TransportDecl> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "`transport` has no kind",
            "write `transport \"i2c\" { .. }`",
        ));
        return None;
    }
    let (kind, kind_span) = names.remove(0);
    if !matches!(kind.as_str(), "i2c" | "spi" | "uart" | "none") {
        errors.push(err(
            kind_span,
            format!("`{kind}` is not a bus kind"),
            "write `transport \"i2c\"`, `\"spi\"`, `\"uart\"` or `\"none\"`",
        ));
        return None;
    }

    let mut bus = None;
    let mut codegen = None;
    let mut dependencies = Vec::new();
    for child in children(node) {
        match child.name().value() {
            "bus" => {
                if bus.is_some() {
                    errors.push(err(node_range(child), "`bus` given twice", "keep one"));
                } else {
                    bus = parse_bus(child, Some(&kind), errors);
                }
            }
            "codegen" => {
                if codegen.is_some() {
                    errors.push(err(node_range(child), "`codegen` given twice", "keep one"));
                } else {
                    codegen = parse_codegen(child, errors);
                }
            }
            "dependency" => {
                if let Some(dependency) = parse_dependency(child, &kind, true, errors) {
                    dependencies.push(dependency);
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `transport \"{kind}\"`"),
                "expected `bus`, `codegen` or `dependency`",
            )),
        }
    }

    let Some(bus) = bus else {
        errors.push(err(
            span.clone(),
            format!("`transport \"{kind}\"` has no `bus`"),
            "write `bus \"<kind>\" { .. }` inside the transport",
        ));
        return None;
    };
    if bus.kind != kind {
        errors.push(err(
            bus.span.clone(),
            format!("`transport \"{kind}\"` declares a `{}` bus", bus.kind),
            "the transport's kind and its bus agree",
        ));
        return None;
    }
    let Some(codegen) = codegen else {
        errors.push(err(
            span.clone(),
            format!("`transport \"{kind}\"` has no `codegen`"),
            "name at least one `runtime \"blocking\"` or `runtime \"async\"` block",
        ));
        return None;
    };
    Some(TransportDecl {
        kind,
        bus,
        codegen,
        dependencies,
        span,
    })
}

/// [`add_bus_attrs`] for one transport of a multi-transport driver.
///
/// The element accepts the union of every transport's bus attributes, so each
/// is optional here: which one is required is decided by the bus the spec binds
/// to, and lowering refuses a missing `address` or `cs` then.
fn add_union_bus_attrs(bus: &BusDecl, attrs: &mut Vec<AttrDef>) {
    match bus.kind.as_str() {
        "i2c" => {
            if matches!(bus.address, AddressNeed::None) && bus.address_allowed.is_none() {
                return;
            }
            attrs.push(AttrDef {
                name: "address".into(),
                kind: AttrKind::Hex,
                required: false,
                default: bus
                    .address_default
                    .map(|address| format!("0x{address:02X}")),
                range: None,
                help: Some("7-bit I2C slave address.".into()),
                span: bus.span.clone(),
            });
        }
        "spi" => {
            attrs.push(AttrDef {
                name: "cs".into(),
                kind: AttrKind::PinOrRef,
                required: false,
                default: None,
                range: None,
                help: Some("Chip-select GPIO number or a <gpio> reference.".into()),
                span: bus.span.clone(),
            });
            if bus.spi_config {
                attrs.push(AttrDef {
                    name: "mode".into(),
                    kind: AttrKind::Choice(vec!["0".into(), "1".into(), "2".into(), "3".into()]),
                    required: false,
                    default: Some(bus.mode.clone().unwrap_or_else(|| "0".into())),
                    range: None,
                    help: Some("SPI mode applied before each transaction.".into()),
                    span: bus.span.clone(),
                });
                attrs.push(AttrDef {
                    name: "frequency".into(),
                    kind: AttrKind::Freq,
                    required: false,
                    default: Some(bus.frequency.clone().unwrap_or_else(|| "40000kHz".into())),
                    range: None,
                    help: Some("SPI clock applied before each transaction.".into()),
                    span: bus.span.clone(),
                });
            }
        }
        _ => {}
    }
}

/// Fold one `transport`-scoped `dependency` into a recipe's dependency set.
///
/// A source-less block (`dependency "bme280" { features "i2c" }`) amends the
/// crate the driver already declares; one that repeats a source must agree with
/// it.
fn absorb_dependency(
    dependencies: &mut Vec<Dependency>,
    dependency: &Dependency,
    tag: &str,
    transport: &TransportDecl,
    errors: &mut Vec<SchemaError>,
) {
    let Some(known) = dependencies
        .iter_mut()
        .find(|known| known.package == dependency.package)
    else {
        if dependency.version.is_none() && dependency.path.is_none() && dependency.git.is_none() {
            errors.push(err(
                transport.span.clone(),
                format!(
                    "`dependency` `{}` in `transport \"{}\"` of `{tag}` names no source",
                    dependency.package, transport.kind
                ),
                "name a `version`, `path` or `git`, or declare the crate at the driver level",
            ));
        } else {
            dependencies.push(dependency.clone());
        }
        return;
    };
    if !known.same_source(dependency) {
        errors.push(err(
            transport.span.clone(),
            format!(
                "`dependency` `{}` in `transport \"{}\"` of `{tag}` names another source",
                dependency.package, transport.kind
            ),
            "a transport adds features to the driver's crate, it does not replace it",
        ));
        return;
    }
    known.absorb(dependency);
}

fn add_bus_attrs(bus: &BusDecl, attrs: &mut Vec<AttrDef>) {
    match bus.kind.as_str() {
        "i2c" => {
            if matches!(bus.address, AddressNeed::None) && bus.address_allowed.is_none() {
                return;
            }
            let (required, default) = match (bus.address, bus.address_default) {
                (AddressNeed::Fixed(_), _) => (true, None),
                (_, Some(address)) => (false, Some(format!("0x{address:02X}"))),
                (AddressNeed::Any, None) => (true, None),
                _ => (true, None),
            };
            attrs.push(AttrDef {
                name: "address".into(),
                kind: AttrKind::Hex,
                required,
                default,
                range: None,
                help: Some("7-bit I2C slave address.".into()),
                span: bus.span.clone(),
            });
        }
        "spi" => {
            attrs.push(AttrDef {
                name: "cs".into(),
                kind: AttrKind::PinOrRef,
                required: true,
                default: None,
                range: None,
                help: Some("Chip-select GPIO number or a <gpio> reference.".into()),
                span: bus.span.clone(),
            });
            if bus.spi_config {
                attrs.push(AttrDef {
                    name: "mode".into(),
                    kind: AttrKind::Choice(vec!["0".into(), "1".into(), "2".into(), "3".into()]),
                    required: false,
                    default: Some(bus.mode.clone().unwrap_or_else(|| "0".into())),
                    range: None,
                    help: Some("SPI mode applied before each transaction.".into()),
                    span: bus.span.clone(),
                });
                attrs.push(AttrDef {
                    name: "frequency".into(),
                    kind: AttrKind::Freq,
                    required: false,
                    default: Some(bus.frequency.clone().unwrap_or_else(|| "40000kHz".into())),
                    range: None,
                    help: Some("SPI clock applied before each transaction.".into()),
                    span: bus.span.clone(),
                });
            }
        }
        _ => {}
    }
}

/// The dependency whose crate the codegen type names.
fn implementation_dependency(
    recipe: &DeviceRecipe,
    dependencies: &[Dependency],
) -> Option<PackageRef> {
    let type_root = recipe.ty.split("::").next().unwrap_or_default().to_string();
    let normalized = |name: &str| name.replace('-', "_");
    let chosen = dependencies
        .iter()
        .find(|dependency| normalized(&dependency.package) == type_root)
        .or_else(|| {
            dependencies
                .iter()
                .find(|dependency| dependency.path.is_some() || dependency.git.is_some())
        })
        .or_else(|| dependencies.first())?;
    Some(PackageRef {
        package: chosen.package.clone(),
        version: chosen.version.clone(),
        path: chosen.path.clone(),
        git: chosen.git.clone(),
        rev: chosen.rev.clone(),
        features: chosen.features.clone(),
    })
}

// ------------------------------------------------------------------- blocks

fn parse_dependency(
    node: &KdlNode,
    tag: &str,
    amendment: bool,
    errors: &mut Vec<SchemaError>,
) -> Option<Dependency> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "`dependency` has no package",
            "write `dependency \"crate-name\" { version \"1\" }`",
        ));
        return None;
    }
    let (package, _) = names.remove(0);
    if package.is_empty()
        || !package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        errors.push(err(
            span,
            format!("`{package}` is not a crate name"),
            "crate names use letters, digits, `-` and `_`",
        ));
        return None;
    }

    let version = child_string(node, "version", errors).map(|(value, _)| value);
    let path = child_string(node, "path", errors).map(|(value, _)| value);
    let git = child_string(node, "git", errors).map(|(value, _)| value);
    let rev = child_string(node, "rev", errors).map(|(value, _)| value);
    let default_features = child_flag(node, "default_features", true);
    let async_default_features = child_flag_option(node, "async_default_features");
    let modes_are_exclusive = child_flag(node, "modes_are_exclusive", false);
    let mut features = Vec::new();
    let mut async_features = Vec::new();

    for child in children(node) {
        match child.name().value() {
            "version" | "path" | "git" | "rev" | "default_features"
            | "async_default_features" | "modes_are_exclusive" => {}
            "features" => features.extend(
                positional_strings(child, errors)
                    .into_iter()
                    .map(|(feature, _)| feature),
            ),
            "async_features" => async_features.extend(
                positional_strings(child, errors)
                    .into_iter()
                    .map(|(feature, _)| feature),
            ),
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `dependency` `{package}`"),
                "expected version, path, git, rev, features, async_features, default_features, \
                 async_default_features or modes_are_exclusive",
            )),
        }
    }

    let chosen = [version.is_some(), path.is_some(), git.is_some()]
        .into_iter()
        .filter(|chosen| *chosen)
        .count();
    // A `transport` block may amend a crate the driver already declares, so
    // it is allowed to name features alone.
    if chosen != 1 && !(amendment && chosen == 0) {
        errors.push(err(
            span.clone(),
            format!("`dependency` `{package}` in `{tag}` names {chosen} sources"),
            "write exactly one of `version`, `path` or `git`",
        ));
        return None;
    }
    if git.is_some() && rev.is_none() {
        errors.push(err(
            span.clone(),
            format!("`dependency` `{package}` names a `git` without a `rev`"),
            "pin the revision, e.g. `rev \"<commit>\"`",
        ));
    }
    if rev.is_some() && git.is_none() {
        errors.push(err(
            span.clone(),
            format!("`dependency` `{package}` names a `rev` without a `git`"),
            "add `git \"<url>\"` or drop `rev`",
        ));
    }

    Some(Dependency {
        package,
        version,
        path,
        git,
        rev,
        features,
        async_features,
        async_default_features,
        default_features,
        modes_are_exclusive,
    })
}

/// The positional boolean of a child node (`fallible #false`), or `default`
/// when the node is absent.
fn child_flag(node: &KdlNode, key: &str, default: bool) -> bool {
    child_flag_option(node, key).unwrap_or(default)
}

/// [`child_flag`] for a flag whose absence is meaningful: `None` keeps the
/// blocking mode's value instead of forcing one.
fn child_flag_option(node: &KdlNode, key: &str) -> Option<bool> {
    for child in children(node) {
        if child.name().value() == key {
            return child
                .entries()
                .iter()
                .find(|entry| entry.name().is_none())
                .map(|entry| entry.value().as_bool() != Some(false));
        }
    }
    None
}

fn parse_bus(
    node: &KdlNode,
    transport_kind: Option<&str>,
    errors: &mut Vec<SchemaError>,
) -> Option<BusDecl> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    // Inside a `transport` block the kind is the transport's own, so `bus
    // { .. }` names no kind and `bus "i2c" { .. }` must agree with it.
    let (kind, kind_span) = if !names.is_empty() {
        names.remove(0)
    } else {
        match transport_kind {
            Some(kind) => (kind.to_string(), span.clone()),
            None => {
                errors.push(err(
                    span,
                    "`bus` has no kind",
                    "write `bus \"i2c\"`, `bus \"spi\"` or `bus \"uart\"`",
                ));
                return None;
            }
        }
    };
    // `none` is a busless driver: its transport is the pins it claims, so it
    // declares no peripheral and no `bus=` attribute.
    if !matches!(kind.as_str(), "i2c" | "spi" | "uart" | "none") {
        errors.push(err(
            kind_span,
            format!("`{kind}` is not a bus kind"),
            "write `bus \"i2c\"`, `bus \"spi\"`, `bus \"uart\"` or `bus \"none\"`",
        ));
        return None;
    }

    let mut bus = BusDecl {
        kind: kind.clone(),
        address: AddressNeed::None,
        address_default: None,
        address_allowed: None,
        spi_config: false,
        mode: None,
        frequency: None,
        cs_polarity: CsPolarity::Low,
        span: span.clone(),
    };

    for child in children(node) {
        match (kind.as_str(), child.name().value()) {
            ("i2c", "address") => {
                let fixed = prop(child, "fixed", errors);
                let default = prop(child, "default", errors);
                let allowed = prop(child, "allowed", errors);
                if fixed.is_some() && default.is_some() {
                    errors.push(err(
                        node_range(child),
                        "`address` names both `fixed` and `default`",
                        "a fixed address is not also a default",
                    ));
                }
                if let Some((value, at)) = &fixed {
                    match hex_byte(value) {
                        Some(address) if USABLE_I2C_ADDRESSES.contains(&address) => {
                            bus.address = AddressNeed::Fixed(address)
                        }
                        _ => errors.push(err(
                            at.clone(),
                            format!("`{value}` is not a usable I2C address"),
                            "write a 7-bit address in 0x08..=0x77",
                        )),
                    }
                } else if let Some((value, at)) = &default {
                    match hex_byte(value) {
                        Some(address) if USABLE_I2C_ADDRESSES.contains(&address) => {
                            bus.address = AddressNeed::Any;
                            bus.address_default = Some(address);
                        }
                        _ => errors.push(err(
                            at.clone(),
                            format!("`{value}` is not a usable I2C address"),
                            "write a 7-bit address in 0x08..=0x77",
                        )),
                    }
                } else {
                    bus.address = AddressNeed::Any;
                }
                if let Some((text, at)) = &allowed {
                    bus.address_allowed = match text.split_once("..=") {
                        Some((low, high)) => match (hex_byte(low.trim()), hex_byte(high.trim())) {
                            (Some(low), Some(high))
                                if low <= high
                                    && USABLE_I2C_ADDRESSES.contains(&low)
                                    && USABLE_I2C_ADDRESSES.contains(&high) =>
                            {
                                Some((low, high))
                            }
                            _ => {
                                errors.push(err(
                                    at.clone(),
                                    format!("`{text}` is not a usable address range"),
                                    "write an inclusive range in 0x08..=0x77, e.g. 0x08..=0x77",
                                ));
                                None
                            }
                        },
                        None => {
                            errors.push(err(
                                at.clone(),
                                format!("`{text}` is not `low..=high`"),
                                "write `allowed \"0x08..=0x77\"`",
                            ));
                            None
                        }
                    };
                }
                if let (AddressNeed::Fixed(fixed), Some((low, high))) =
                    (bus.address, bus.address_allowed)
                    && !(low..=high).contains(&fixed)
                {
                    errors.push(err(
                        node_range(child),
                        format!(
                            "fixed address 0x{fixed:02X} is outside 0x{low:02X}..=0x{high:02X}"
                        ),
                        "widen `allowed`, or fix an address inside it",
                    ));
                }
            }
            ("spi", "mode") => {
                let value = child
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .and_then(|entry| {
                        entry
                            .value()
                            .as_integer()
                            .map(|number| number.to_string())
                            .or_else(|| entry.value().as_string().map(str::to_string))
                    });
                match value {
                    Some(value) if matches!(value.as_str(), "0" | "1" | "2" | "3") => {
                        bus.mode = Some(value)
                    }
                    _ => errors.push(err(
                        node_range(child),
                        "`mode` must be 0, 1, 2 or 3",
                        "write `mode 0`",
                    )),
                }
            }
            ("spi", "frequency") => {
                if let Some((value, _)) = positional_strings(child, errors).into_iter().next() {
                    bus.frequency = Some(value);
                } else {
                    errors.push(err(
                        node_range(child),
                        "`frequency` takes a string",
                        "write `frequency \"40MHz\"`",
                    ));
                }
            }
            ("spi", "cs_polarity") => {
                if let Some((value, at)) = positional_strings(child, errors).into_iter().next() {
                    match value.as_str() {
                        "low" | "active-low" => bus.cs_polarity = CsPolarity::Low,
                        "high" | "active-high" => bus.cs_polarity = CsPolarity::High,
                        other => errors.push(err(
                            at,
                            format!("`{other}` is not a chip-select polarity"),
                            "write `cs_polarity \"active-low\"` or `\"active-high\"`",
                        )),
                    }
                }
            }
            ("spi", "cs_via") => {
                if let Some((value, at)) = positional_strings(child, errors).into_iter().next()
                    && value != "pin_or_ref"
                {
                    errors.push(err(
                        at,
                        format!("`{value}` is not a chip-select form"),
                        "write `cs_via \"pin_or_ref\"`",
                    ));
                }
            }
            ("spi", other) => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `bus \"spi\"`"),
                "expected mode, frequency, cs_polarity or cs_via",
            )),
            ("i2c", other) => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `bus \"i2c\"`"),
                "expected `address`",
            )),
            ("uart", other) => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `bus \"uart\"`"),
                "a uart bus has no bus-level configuration",
            )),
            ("none", other) => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `bus \"none\"`"),
                "a busless driver has no bus-level configuration",
            )),
            _ => {}
        }
    }

    bus.spi_config = bus.mode.is_some() || bus.frequency.is_some();
    Some(bus)
}

fn parse_pin(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<PinDecl> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "`pin` has no name", "write `pin \"dc\" ..`"));
        return None;
    }
    let (name, _) = names.remove(0);
    let direction = match prop(node, "direction", errors) {
        Some((direction, at)) => match direction.as_str() {
            "output" => PinDirection::Output,
            "input" => PinDirection::Input,
            "bidirectional" => PinDirection::Bidirectional,
            "open_drain" | "open-drain" => PinDirection::OpenDrain,
            other => {
                errors.push(err(
                    at,
                    format!("`{other}` is not a pin direction"),
                    "write `direction=\"output\"`, `\"input\"`, `\"bidirectional\"` or \
                     `\"open_drain\"`",
                ));
                PinDirection::Output
            }
        },
        None => PinDirection::Output,
    };
    let level = match prop(node, "default_level", errors) {
        Some((level, at)) => {
            if !matches!(direction, PinDirection::Output | PinDirection::OpenDrain) {
                errors.push(err(
                    at.clone(),
                    "`default_level` is for a driven pin",
                    "an input starts at its pull resistor and a bidirectional line \
                     starts released; drop `default_level`",
                ));
            }
            match level.as_str() {
                "low" => Level::Low,
                "high" => Level::High,
                other => {
                    errors.push(err(
                        at,
                        format!("`{other}` is not a pin level"),
                        "write `default_level=\"low\"` or `\"high\"`",
                    ));
                    Level::Low
                }
            }
        }
        None => Level::Low,
    };
    let mut pull = Pull::None;
    if let Some((spelling, at)) = prop(node, "pull", errors) {
        pull = match spelling.as_str() {
            "up" => Pull::Up,
            "down" => Pull::Down,
            "none" => Pull::None,
            other => {
                errors.push(err(
                    at.clone(),
                    format!("`{other}` is not a pin pull"),
                    "write `pull=\"up\"`, `\"down\"` or `\"none\"`",
                ));
                Pull::None
            }
        };
        if matches!(direction, PinDirection::Output) {
            errors.push(err(
                at,
                format!("`pull` is for an input or bidirectional line, but `{name}` is an output"),
                "write `direction=\"input\"` or `\"bidirectional\"`, or drop `pull`",
            ));
        }
    }
    let optional = node
        .entries()
        .iter()
        .filter(|entry| entry.name().is_some_and(|name| name.value() == "optional"))
        .any(|entry| entry.value().as_bool() != Some(false));

    for child in children(node) {
        errors.push(err(
            node_range(child),
            format!("unknown node `{}` in `pin` `{name}`", child.name().value()),
            "a pin is a name plus `direction=`, `default_level=`, `pull=` and `optional=`",
        ));
    }

    let ref_only = match prop(node, "via", errors) {
        Some((spelling, at)) => match spelling.as_str() {
            "ref" => true,
            "pin_or_ref" => false,
            other => {
                errors.push(err(
                    at,
                    format!("`{other}` is not a pin form"),
                    "write `via=\"ref\"` for a `$<gpio>` reference, or drop it",
                ));
                false
            }
        },
        None => false,
    };

    Some(PinDecl {
        name,
        direction,
        level,
        pull,
        optional,
        ref_only,
        span,
    })
}

/// Whether a `compose` block reads `name` from the composite.
fn part_uses_pin(parts: &[RecipePart], name: &str) -> bool {
    parts.iter().any(|part| {
        part.attrs
            .iter()
            .any(|attr| matches!(&attr.source, PartSource::From(from) if from == name))
    })
}

/// Parse one `compose "<tag>" { attr "<name>" from=".." }` block.
///
/// The block names a companion driver and fills its attributes: `from=` reads
/// one of the composite's own attributes (a pin), `value=` is a literal. What
/// the part is and whether the mapping is complete is checked once every
/// source is parsed, so a manifest may compose a driver declared later.
fn parse_compose(node: &KdlNode, parts: &mut Vec<RecipePart>, errors: &mut Vec<SchemaError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "`compose` has no tag",
            "write `compose \"<driver tag>\" { .. }`",
        ));
        return;
    }
    let (tag, tag_span) = names.remove(0);
    if parts.iter().any(|part| part.tag == tag) {
        errors.push(err(
            tag_span,
            format!("`compose \"{tag}\"` given twice"),
            "each companion driver is composed once",
        ));
        return;
    }

    let mut attrs: Vec<PartAttr> = Vec::new();
    for child in children(node) {
        if child.name().value() != "attr" {
            errors.push(err(
                node_range(child),
                format!(
                    "unknown node `{}` in `compose` `{tag}`",
                    child.name().value()
                ),
                "expected `attr \"<name>\" from=\"..\"` or `attr \"<name>\" value=\"..\"`",
            ));
            continue;
        }
        let attr_span = node_range(child);
        let mut attr_names = positional_strings(child, errors);
        if attr_names.is_empty() {
            errors.push(err(
                attr_span,
                "`attr` has no name",
                "write `attr \"cs\" from=\"touch_cs\"`",
            ));
            continue;
        }
        let (name, _) = attr_names.remove(0);
        if attrs.iter().any(|known| known.name == name) {
            errors.push(err(
                attr_span,
                format!("`{tag}.{name}` mapped twice"),
                "each part attribute is filled once",
            ));
            continue;
        }
        let from = prop(child, "from", errors);
        let value = prop(child, "value", errors);
        let source = match (from, value) {
            (Some((from, _)), None) => PartSource::From(from),
            (None, Some((value, _))) => PartSource::Value(value),
            (Some(_), Some(_)) => {
                errors.push(err(
                    attr_span,
                    format!("`{tag}.{name}` names both `from` and `value`"),
                    "read it from the composite (`from=`) or fix it (`value=`), not both",
                ));
                continue;
            }
            (None, None) => {
                errors.push(err(
                    attr_span,
                    format!("`{tag}.{name}` names no source"),
                    "write `from=\"<composite attr>\"` or `value=\"<literal>\"`",
                ));
                continue;
            }
        };
        attrs.push(PartAttr {
            name,
            source,
            span: attr_span,
        });
    }

    parts.push(RecipePart { tag, attrs, span });
}

fn parse_codegen(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<Codegen> {
    let span = node_range(node);
    let mut codegen = Codegen {
        span: span.clone(),
        fallible: true,
        ..Codegen::default()
    };
    for child in children(node) {
        match child.name().value() {
            "feature" => match positional_strings(child, errors).into_iter().next() {
                Some((feature, _)) => {
                    if codegen.feature.is_some() {
                        errors.push(err(node_range(child), "`feature` given twice", "keep one"));
                    } else if feature.is_empty() {
                        errors.push(err(
                            node_range(child),
                            "`feature` is empty",
                            "name the `espforge-drivers` feature",
                        ));
                    } else {
                        codegen.feature = Some(feature);
                    }
                }
                None => errors.push(err(
                    node_range(child),
                    "`feature` takes a string",
                    "write `feature \"ssd1306\"`",
                )),
            },
            "runtime" => {
                let value = child
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .and_then(|entry| entry.value().as_string());
                match value {
                    Some("blocking") => {
                        if codegen.blocking.is_some() {
                            errors.push(err(
                                node_range(child),
                                "`runtime \"blocking\"` given twice",
                                "keep one",
                            ));
                        } else {
                            codegen.blocking = parse_runtime(child, false, errors);
                        }
                    }
                    Some("async") => {
                        if codegen.asynchronous.is_some() {
                            errors.push(err(
                                node_range(child),
                                "`runtime \"async\"` given twice",
                                "keep one",
                            ));
                        } else {
                            codegen.asynchronous = parse_runtime(child, true, errors);
                        }
                    }
                    _ => errors.push(err(
                        node_range(child),
                        "`runtime` must be `blocking` or `async`",
                        "write `runtime \"blocking\" { .. }`",
                    )),
                }
            }
            // `fallible #false`: the constructor returns the device, not a
            // `Result`, so the generated call maps no error.
            "fallible" => {
                codegen.fallible = child
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .map(|entry| entry.value().as_bool() != Some(false))
                    .unwrap_or(true);
            }
            "map" => {
                if let Some(map) = parse_map(child, errors) {
                    if codegen.maps.iter().any(|known| known.name == map.name) {
                        errors.push(err(
                            map.span,
                            format!("duplicate `map` `{}`", map.name),
                            "each attribute has one map",
                        ));
                    } else {
                        codegen.maps.push(map);
                    }
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `codegen`"),
                "expected feature, fallible, runtime or map",
            )),
        }
    }
    Some(codegen)
}

fn parse_runtime(
    node: &KdlNode,
    asynchronous: bool,
    errors: &mut Vec<SchemaError>,
) -> Option<RuntimeDecl> {
    let span = node_range(node);
    let mut runtime = RuntimeDecl {
        span: span.clone(),
        ..RuntimeDecl::default()
    };
    for child in children(node) {
        match child.name().value() {
            "type" => {
                if let Some((value, at)) = positional_strings(child, errors).into_iter().next() {
                    if runtime.ty.is_some() {
                        errors.push(err(at, "`type` given twice", "keep one"));
                    } else {
                        runtime.ty = Some((value, at));
                    }
                }
            }
            "constructor" => {
                if let Some((value, at)) = positional_strings(child, errors).into_iter().next() {
                    if runtime.constructor.is_some() {
                        errors.push(err(at, "`constructor` given twice", "keep one"));
                    } else {
                        runtime.constructor = Some((value, at));
                    }
                }
            }
            "endpoint" => {
                if let Some((value, at)) = positional_strings(child, errors).into_iter().next() {
                    // The endpoint is the type the generated bus helper already
                    // returns, not a template: a generic type is the normal
                    // shape, and there is nothing for a `{placeholder}` to
                    // fill.
                    match first_placeholder(&value) {
                        Some(placeholder) => errors.push(err(
                            at,
                            "`endpoint` takes no `{placeholder}`",
                            format!(
                                "remove `{{{placeholder}}}`: an endpoint is a type, not a template"
                            ),
                        )),
                        None => {
                            validate_placeholders(&value, &[], at.clone(), "endpoint", errors);
                            validate_type_in(&value, "endpoint", at, errors);
                            runtime.endpoint = Some(value);
                        }
                    }
                }
            }
            "feature" => {
                if let Some((value, at)) = positional_strings(child, errors).into_iter().next() {
                    if value.is_empty() {
                        errors.push(err(at, "`feature` is empty", "name the async feature"));
                    } else {
                        runtime.feature = Some(value);
                    }
                }
            }
            // `awaits #false`: the async constructor is a plain `fn`, not an
            // `async fn`. Absent means awaited, which is every bundled driver.
            "awaits" => {
                if !asynchronous {
                    errors.push(err(
                        node_range(child),
                        "`awaits` belongs to the async runtime",
                        "a blocking constructor is a plain `fn`",
                    ));
                } else {
                    runtime.awaits = Some(
                        child
                            .entries()
                            .iter()
                            .find(|entry| entry.name().is_none())
                            .map(|entry| entry.value().as_bool() != Some(false))
                            .unwrap_or(true),
                    );
                }
            }
            // `then { call "init({delay})" awaits #true }`: a method call the
            // device runs after its constructor returns, in the order written.
            "then" => {
                if let Some(step) = parse_step(child, asynchronous, errors) {
                    runtime.steps.push(step);
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `runtime`"),
                "expected type, constructor, endpoint, feature, awaits or then",
            )),
        }
    }
    if runtime.ty.is_none() {
        errors.push(err(
            span.clone(),
            "`runtime` has no `type`",
            "name the generated field's Rust type",
        ));
    }
    if runtime.constructor.is_none() {
        errors.push(err(
            span,
            "`runtime` has no `constructor`",
            "name the constructor call",
        ));
    }
    Some(runtime)
}

/// One `then` step: a single method call on the device the constructor built.
///
/// The receiver is the generated local, never text: a manifest may name the
/// order of calls and their arguments, and nothing else.
fn parse_step(
    node: &KdlNode,
    asynchronous: bool,
    errors: &mut Vec<SchemaError>,
) -> Option<StepDecl> {
    let span = node_range(node);
    let mut call_text = None;
    let mut saw_call = false;
    // A blocking step is a plain call; an async one is awaited unless it says
    // the crate's `init` is a synchronous `fn`.
    let mut awaits = asynchronous;
    // The result is dropped unless the call rebinds the device (a typestate
    // `init(self)`), and it is mapped to `Init` unless the call has no error.
    let mut rebind = false;
    let mut fallible = true;
    for child in children(node) {
        match child.name().value() {
            "call" => {
                saw_call = true;
                // `awaits` is a line of its own, as it is in `runtime`; writing
                // it beside the string would make it a second argument.
                if child
                    .entries()
                    .iter()
                    .filter(|entry| entry.name().is_none())
                    .any(|entry| entry.value().as_string() == Some("awaits"))
                {
                    errors.push(err(
                        node_range(child),
                        "`awaits` is a step line, not part of `call`",
                        "keep `call \"..\"` and write `awaits #false` on the next line",
                    ));
                    continue;
                }
                let Some((text, at)) = positional_strings(child, errors).into_iter().next() else {
                    errors.push(err(
                        node_range(child),
                        "`call` takes a string",
                        "write `call \"init({delay})\"`",
                    ));
                    continue;
                };
                if call_text.is_some() {
                    errors.push(err(
                        node_range(child),
                        "`then` has two `call`s",
                        "one method call per `then`",
                    ));
                    continue;
                }
                if text.trim_start().starts_with("self.") {
                    errors.push(err(
                        at,
                        "a `then` step takes no receiver",
                        "write `call \"init({delay})\"`: the device is the receiver",
                    ));
                    continue;
                }
                match crate::syntax::step_call(&text) {
                    Ok(call) => {
                        if !matches!(
                            call.as_syn(),
                            Expr::MethodCall(method) if crate::syntax::is_step_receiver(&method.receiver)
                        ) {
                            errors.push(err(
                                at,
                                "a `then` step must be one method call on the device",
                                "write `call \"init({delay})\"`: the step names no receiver",
                            ));
                            continue;
                        }
                    }
                    Err((message, label)) => {
                        errors.push(err(at, message, label));
                        continue;
                    }
                }
                call_text = Some(text);
            }
            // `awaits #false`: a step's call is a plain `fn`, the same flag
            // the constructor's own `awaits` uses.
            "awaits" => {
                if !asynchronous {
                    errors.push(err(
                        node_range(child),
                        "`awaits` belongs to the async runtime",
                        "a blocking step is a plain method call",
                    ));
                }
                awaits = asynchronous
                    && child
                        .entries()
                        .iter()
                        .find(|entry| entry.name().is_none())
                        .map(|entry| entry.value().as_bool() != Some(false))
                        .unwrap_or(true);
            }
            // `rebind #true`: the call consumes the device and returns the one
            // the generated field stores (`let mut dev = dev.init()?`).
            "rebind" => {
                rebind = child
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .map(|entry| entry.value().as_bool() != Some(false))
                    .unwrap_or(true);
            }
            // `fallible #false`: the call returns `()` or `&mut Self`, so the
            // generated statement must not map an error that does not exist.
            "fallible" => {
                fallible = child
                    .entries()
                    .iter()
                    .find(|entry| entry.name().is_none())
                    .map(|entry| entry.value().as_bool() != Some(false))
                    .unwrap_or(true);
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `then`"),
                "write `then { call \"init({delay})\" }`",
            )),
        }
    }
    if call_text.is_none() && !saw_call {
        errors.push(err(
            span.clone(),
            "`then` has no `call`",
            "write `then { call \"init({delay})\" }`",
        ));
    }
    call_text.map(|text| StepDecl {
        text,
        awaits,
        rebind,
        fallible,
        span,
    })
}

/// The `then` steps of one runtime block, checked against the constructor.
///
/// A step resolves from the constructor's own arguments, so every hole it
/// names must be one of them: a step has no other way to reach a value.
fn parse_steps(
    runtime: Option<&RuntimeDecl>,
    args: &[crate::CtorArg],
    errors: &mut Vec<SchemaError>,
) -> Vec<crate::Step> {
    let Some(runtime) = runtime else {
        return Vec::new();
    };
    let known: Vec<&str> = args
        .iter()
        .filter_map(|arg| match arg {
            crate::CtorArg::Bus => None,
            crate::CtorArg::Address => Some("address"),
            crate::CtorArg::Slot(arg) => Some(arg.name.as_str()),
            crate::CtorArg::Fixed { .. } => None,
        })
        .collect();
    let mut steps = Vec::new();
    for decl in &runtime.steps {
        let mut usable = true;
        let names = crate::syntax::substitute(&decl.text)
            .map(|(_, names)| names)
            .unwrap_or_default();
        for name in names {
            // `{dc_pin}` is the spelling the phase-8 format uses; `{dc}` is
            // the pin's own name, and both mean the same claimed line.
            let pin = name.strip_suffix("_pin").unwrap_or(name.as_str());
            if !known.iter().any(|arg| *arg == name.as_str() || *arg == pin) {
                errors.push(err(
                    decl.span.clone(),
                    format!("`{{{name}}}` in a `then` step is not a constructor argument"),
                    "a step reads only the values the constructor already takes",
                ));
                usable = false;
            }
        }
        if usable {
            steps.push(crate::Step {
                text: decl.text.clone(),
                awaits: decl.awaits,
                rebind: decl.rebind,
                fallible: decl.fallible,
                span: decl.span.clone(),
            });
        }
    }
    steps
}

fn parse_map(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<MapDecl> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "`map` has no name",
            "write `map \"size\" { .. }`",
        ));
        return None;
    }
    let (name, _) = names.remove(0);
    let mut entries = Vec::new();
    for child in children(node) {
        let spelling = child.name().value().to_string();
        if entries.iter().any(|(known, _)| *known == spelling) {
            errors.push(err(
                node_range(child),
                format!("duplicate map entry `{spelling}`"),
                "map each spelling once",
            ));
            continue;
        }
        let mut value = None;
        for entry in children(child) {
            let kind = entry.name().value();
            if !matches!(kind, "type" | "expr") {
                errors.push(err(
                    node_range(entry),
                    format!("unknown node `{kind}` in map entry"),
                    "write `type \"..\"` or `expr \"..\"`",
                ));
                continue;
            }
            let Some((text, at)) = positional_strings(entry, errors).into_iter().next() else {
                errors.push(err(
                    node_range(entry),
                    format!("`{kind}` takes a string"),
                    "quote it",
                ));
                continue;
            };
            if value.is_some() {
                errors.push(err(at, "map entry has both `type` and `expr`", "keep one"));
            } else {
                value = Some(if kind == "type" {
                    MapValue::Type(text)
                } else {
                    MapValue::Expr(text)
                });
            }
        }
        match value {
            Some(value) => entries.push((spelling, value)),
            None => errors.push(err(
                node_range(child),
                format!("map entry `{spelling}` has no value"),
                "write `type \"..\"` or `expr \"..\"`",
            )),
        }
    }
    Some(MapDecl {
        name,
        entries,
        span,
    })
}

// --------------------------------------------------------------- compiling

#[allow(clippy::too_many_arguments)]
fn build_recipe(
    tag: &str,
    bus: &BusDecl,
    pins: &[PinDecl],
    attrs: &[AttrDef],
    codegen: &Codegen,
    dependencies: &[Dependency],
    parts: &[RecipePart],
    errors: &mut Vec<SchemaError>,
) -> Option<DeviceRecipe> {
    if codegen.blocking.is_none() && codegen.asynchronous.is_none() {
        errors.push(err(
            codegen.span.clone(),
            format!("`codegen` in `{tag}` declares no runtime"),
            "add `runtime \"blocking\" { .. }` or `runtime \"async\" { .. }`",
        ));
        return None;
    }
    if codegen.feature.is_none() && dependencies.is_empty() {
        errors.push(err(
            codegen.span.clone(),
            format!("`{tag}` names no crate"),
            "write `codegen { feature \"..\" }` or a `dependency` block",
        ));
    }
    // A bundled driver returns `DeviceErrorKind` and the `espforge-drivers`
    // API is shaped around that; only a crate the manifest names can return
    // the device directly.
    if !codegen.fallible && codegen.feature.is_some() {
        errors.push(err(
            codegen.span.clone(),
            "`fallible #false` needs a crate that returns the device directly",
            "drop `feature \"..\"` or the flag",
        ));
    }

    // `address` is not an attribute: it belongs to the bus. A map turns each
    // address the driver allows into the crate's own value
    // (`pcf857x::SlaveAddr`), so the constructor can take the crate's type
    // while the element keeps writing a plain 7-bit address.
    let mut address_map: Option<Vec<(u8, String)>> = None;
    if let Some(map) = codegen.maps.iter().find(|map| map.name == "address") {
        match bus.address {
            AddressNeed::None => errors.push(err(
                map.span.clone(),
                format!("`map \"address\"` in `{tag}` names no I2C address"),
                "add an `address` to `bus \"i2c\"`, or drop the map",
            )),
            _ => {
                let required: Vec<u8> = match bus.address {
                    AddressNeed::Fixed(fixed) => vec![fixed],
                    _ => match bus.address_allowed {
                        Some((low, high)) => (low..=high).collect(),
                        None => {
                            errors.push(err(
                                map.span.clone(),
                                format!("`map \"address\"` in `{tag}` has no address range"),
                                "declare `address allowed=\"0x..=0x..\"` so the map can be complete",
                            ));
                            Vec::new()
                        }
                    },
                };
                let mut entries: Vec<(u8, String)> = Vec::new();
                for (spelling, value) in &map.entries {
                    let Some(address) = crate::hex_byte(spelling) else {
                        errors.push(err(
                            map.span.clone(),
                            format!("`{spelling}` is not an I2C address"),
                            "write the address the element declares, for example `\"0x20\"`",
                        ));
                        continue;
                    };
                    if !required.contains(&address) {
                        errors.push(err(
                            map.span.clone(),
                            format!("`0x{address:02X}` is not an address of `{tag}`"),
                            match bus.address_allowed {
                                Some((low, high)) => {
                                    format!("addresses: 0x{low:02X}..=0x{high:02X}")
                                }
                                None => format!("the part is fixed at 0x{address:02X}"),
                            },
                        ));
                        continue;
                    }
                    if entries.iter().any(|(known, _)| *known == address) {
                        errors.push(err(
                            map.span.clone(),
                            format!("`0x{address:02X}` is mapped twice"),
                            "write each address once, in hex or decimal",
                        ));
                        continue;
                    }
                    let MapValue::Expr(text) = value else {
                        errors.push(err(
                            map.span.clone(),
                            format!("`map \"address\"` entry `{spelling}` is a type"),
                            "an address fills a value: write `expr \"..\"`",
                        ));
                        continue;
                    };
                    validate_expr(text, map.span.clone(), errors);
                    entries.push((address, text.clone()));
                }
                for address in required {
                    if !entries.iter().any(|(known, _)| *known == address) {
                        errors.push(err(
                            map.span.clone(),
                            format!("`map \"address\"` has no entry for `0x{address:02X}`"),
                            "every address the driver allows needs an entry",
                        ));
                    }
                }
                address_map = Some(entries);
            }
        }
    }

    let maps: BTreeMap<&str, &MapDecl> = codegen
        .maps
        .iter()
        .filter(|map| map.name != "address")
        .map(|map| (map.name.as_str(), map))
        .collect();

    for map in codegen.maps.iter().filter(|map| map.name != "address") {
        let Some(attr) = attrs.iter().find(|attr| attr.name == map.name) else {
            errors.push(err(
                map.span.clone(),
                format!("`map` names no attribute `{tag}.{}`", map.name),
                "a map fills one of the driver's `attr`s",
            ));
            continue;
        };
        let spellings: Vec<String> = match &attr.kind {
            AttrKind::Choice(choices) => choices.clone(),
            AttrKind::Bool => vec!["true".into(), "false".into()],
            other => {
                errors.push(err(
                    map.span.clone(),
                    format!("`{tag}.{}` is `{}`", map.name, other.display()),
                    "a map fills a `choice` or `bool` attribute",
                ));
                continue;
            }
        };
        for spelling in &spellings {
            if !map.entries.iter().any(|(known, _)| known == spelling) {
                errors.push(err(
                    map.span.clone(),
                    format!("`map \"{}\"` has no entry for `{spelling}`", map.name),
                    "every value the attribute allows needs an entry",
                ));
            }
        }
        for (spelling, value) in &map.entries {
            if !spellings.contains(spelling) {
                errors.push(err(
                    map.span.clone(),
                    format!("`{spelling}` is not a value of `{tag}.{}`", map.name),
                    format!("choices: {}", spellings.join(", ")),
                ));
            }
            match value {
                MapValue::Type(text) => validate_type(text, map.span.clone(), errors),
                MapValue::Expr(text) => validate_expr(text, map.span.clone(), errors),
            }
        }
    }

    let primary = codegen
        .blocking
        .as_ref()
        .or(codegen.asynchronous.as_ref())
        .expect("one runtime is present");
    let (ty, ty_span) = primary.ty.clone()?;
    let (constructor, ctor_span) = primary.constructor.clone()?;

    let type_names: Vec<&str> = maps.keys().copied().collect();
    validate_type(&ty, ty_span.clone(), errors);
    validate_placeholders(&ty, &type_names, ty_span.clone(), "type", errors);

    let (new, ctor_args) = parse_constructor(
        &constructor,
        ctor_span.clone(),
        &ConstructorCtx {
            tag,
            bus,
            pins,
            attrs,
            maps: &maps,
        },
        errors,
    )?;
    // The address keeps the place the manifest gave it: directly after the
    // bus, either in the wrapper that takes the bus or in the argument after
    // it. `parse_constructor` drops a plain bus argument, so a wrapped bus is
    // only ever one argument long at the front.
    let address_at = ctor_args.iter().position(|spec| spec.uses("address"));
    match (bus.address, address_at.is_some()) {
        // A part the project addresses must hand that address to its
        // constructor.
        (AddressNeed::Any, false) => errors.push(err(
            ctor_span.clone(),
            format!("`{tag}` declares an address but its constructor does not take it"),
            "add `{address}` to the constructor",
        )),
        // A part at one `fixed` address may hard-code it in the crate, so the
        // manifest may leave `{address}` out; the element still declares the
        // address, so lowering routes and checks it.
        (AddressNeed::Fixed(_), _) => {}
        (AddressNeed::None, true) => errors.push(err(
            ctor_span.clone(),
            format!("`{tag}` has no I2C address but its constructor takes `{{address}}`"),
            "add an `address` to `bus \"i2c\"` or drop the argument",
        )),
        _ => {}
    }
    if address_map.is_some() && address_at.is_none() {
        errors.push(err(
            ctor_span.clone(),
            format!("`map \"address\"` in `{tag}` is not used"),
            "the constructor takes no `{address}`, so nothing reads the map",
        ));
    }

    let mut args: Vec<crate::CtorArg> = Vec::new();
    let mut used_pins: Vec<&str> = Vec::new();
    for spec in ctor_args {
        let (name, at) = match spec {
            // The bus keeps its position: emission inserts the endpoint here.
            CtorSpec::Placeholder { name, .. }
                if matches!(name.as_str(), "bus_device" | "spi_device") =>
            {
                args.push(crate::CtorArg::Bus);
                continue;
            }
            // The address is a constructor argument like any other, and level
            // lowering fills it from the element.
            CtorSpec::Placeholder { name, .. } if name == "address" => {
                args.push(crate::CtorArg::Address);
                continue;
            }
            // A fixed expression keeps the text the manifest wrote; no slot
            // resolves it and no attribute can change it.
            CtorSpec::Fixed { text, span } => {
                args.push(crate::CtorArg::Fixed { text, span });
                continue;
            }
            CtorSpec::Placeholder { name, span } => (name, span),
        };
        if let Some(pin) = pins.iter().find(|pin| pin.name == name) {
            used_pins.push(&pin.name);
            args.push(crate::CtorArg::Slot(DeviceArg {
                name: pin.name.clone(),
                map: None,
                pin: Some(pin_template(pin.direction, pin.level, pin.pull)),
                direction: pin.direction,
                open_drain: matches!(pin.direction, PinDirection::OpenDrain),
                level: Some(pin.level.word()),
                pull: Some(pin.pull.word()),
                span: at,
            }));
            continue;
        }
        if name == "cs_pin" {
            continue;
        }
        let Some(map) = maps.get(name.as_str()) else {
            errors.push(err(
                at.clone(),
                format!("`{{{name}}}` is not a mapped attribute or a declared pin"),
                "declare it with `attr`/`map` or `pin`, or drop it from the constructor",
            ));
            continue;
        };
        let entries = map
            .entries
            .iter()
            .map(|(spelling, value)| {
                let text = match value {
                    MapValue::Type(text) | MapValue::Expr(text) => text.clone(),
                };
                (spelling.clone(), text)
            })
            .collect();
        args.push(crate::CtorArg::Slot(DeviceArg {
            name: map.name.clone(),
            map: Some(entries),
            pin: None,
            direction: PinDirection::Output,
            open_drain: false,
            level: None,
            pull: None,
            span: at,
        }));
    }
    for pin in pins {
        if used_pins.contains(&pin.name.as_str()) || part_uses_pin(parts, &pin.name) {
            continue;
        }
        errors.push(err(
            pin.span.clone(),
            format!("`pin \"{}\"` is not a constructor argument", pin.name),
            format!(
                "add `{{{}}}` to the constructor, or map it in a `compose` block",
                pin.name
            ),
        ));
    }
    // A `then` step runs on the device the constructor built, so it may only
    // name values the constructor already takes: emission fills a step from
    // the same arguments, and there is nothing else it could reach.
    let steps = parse_steps(codegen.blocking.as_ref(), &args, errors);
    let async_steps = parse_steps(codegen.asynchronous.as_ref(), &args, errors);
    let (runtimes, mut async_mode) = match (&codegen.blocking, &codegen.asynchronous) {
        (Some(_), Some(runtime)) => {
            let (async_ty, async_span) = runtime.ty.clone()?;
            validate_type(&async_ty, async_span.clone(), errors);
            validate_placeholders(&async_ty, &type_names, async_span.clone(), "type", errors);
            let (async_new, async_args) =
                runtime.constructor.clone().and_then(|(ctor, span)| {
                    parse_constructor(
                        &ctor,
                        span,
                        &ConstructorCtx {
                            tag,
                            bus,
                            pins,
                            attrs,
                            maps: &maps,
                        },
                        errors,
                    )
                })?;
            // Two constructors of one driver must take the same things in the
            // same order, constants included: a dual-mode driver whose halves
            // ask for different values is a manifest bug.
            let async_args: Vec<ArgShape<'_>> = async_args
                .iter()
                .filter_map(CtorSpec::shape)
                .collect();
            let blocking_args: Vec<ArgShape<'_>> = args
                .iter()
                .filter_map(crate::CtorArg::shape)
                .collect();
            if blocking_args != async_args {
                errors.push(err(
                    runtime.span.clone(),
                    format!("the async constructor of `{tag}` takes different arguments"),
                    "both constructors must take the same bus, pins, slots and fixed values",
                ));
            }
            (
                Runtimes {
                    blocking: true,
                    asynchronous: true,
                },
                AsyncMode {
                    ty: Some(async_ty),
                    new: Some(async_new),
                    feature: runtime.feature.clone(),
                    endpoint: runtime.endpoint.clone(),
                    steps: Vec::new(),
                    awaits: runtime.awaits.unwrap_or(true),
                },
            )
        }
        (Some(_), None) => (Runtimes::blocking(), AsyncMode::default()),
        (None, Some(runtime)) => {
            let (async_ty, async_span) = runtime.ty.clone()?;
            validate_type(&async_ty, async_span.clone(), errors);
            validate_placeholders(&async_ty, &type_names, async_span.clone(), "type", errors);
            let endpoint = runtime.endpoint.clone();
            (
                Runtimes {
                    blocking: false,
                    asynchronous: true,
                },
                AsyncMode {
                    ty: Some(async_ty),
                    new: Some(new.clone()),
                    feature: runtime.feature.clone(),
                    endpoint: endpoint.clone(),
                    steps: Vec::new(),
                    awaits: runtime.awaits.unwrap_or(true),
                },
            )
        }
        (None, None) => unreachable!("checked above"),
    };
    async_mode.steps = async_steps;

    Some(DeviceRecipe {
        bus: bus.kind.clone(),
        feature: codegen.feature.clone().unwrap_or_default(),
        kind: "device".into(),
        ty,
        new,
        fallible: codegen.fallible,
        deps: dependencies
            .iter()
            .map(|dependency| dependency.package.clone())
            .collect(),
        args,
        steps,
        parts: parts.to_vec(),
        runtimes,
        async_mode,
        endpoint: codegen
            .blocking
            .as_ref()
            .and_then(|runtime| runtime.endpoint.clone())
            .or_else(|| {
                codegen
                    .asynchronous
                    .as_ref()
                    .and_then(|runtime| runtime.endpoint.clone())
            }),
        spi_config: bus.spi_config,
        cs_polarity: bus.cs_polarity,
        address: bus.address,
        address_default: bus.address_default,
        address_allowed: bus.address_allowed,
        address_map,
        dependencies: Vec::new(),
        transports: Vec::new(),
        span: codegen.span.clone(),
    })
}

type ConstructorArgs = (String, Vec<CtorSpec>);

/// One constructor argument after the bus, before it becomes a
/// [`crate::CtorArg`].
///
/// The split happens here and not downstream because only the manifest knows
/// whether an argument is a hole the element fills or a value it fixed.
enum CtorSpec {
    /// A `{name}` hole, with the name it resolved to.
    Placeholder { name: String, span: Range<usize> },
    /// A fixed Rust expression the manifest wrote.
    Fixed { text: String, span: Range<usize> },
}

impl CtorSpec {
    /// Whether this argument fills `name`, as a slot or inside an expression.
    fn uses(&self, name: &str) -> bool {
        match self {
            CtorSpec::Placeholder { name: known, .. } => known == name,
            CtorSpec::Fixed { text, .. } => crate::syntax::uses_placeholder(text, name),
        }
    }

    /// What this argument contributes, for comparing the blocking and async
    /// constructors: a slot name or a fixed expression.
    fn shape(&self) -> Option<ArgShape<'_>> {
        match self {
            CtorSpec::Placeholder { name, .. }
                if matches!(name.as_str(), "address" | "bus_device" | "spi_device") =>
            {
                None
            }
            CtorSpec::Placeholder { name, .. } => Some((Some(name.as_str()), None)),
            CtorSpec::Fixed { text, .. } => Some((None, Some(text.as_str()))),
        }
    }
}

/// The holes a fixed argument may carry inside an expression.
///
/// Every other slot is a whole argument: a probe line is claimed and deferred on
/// its own, and a mapped value is resolved by the position it sits in. The bus
/// and the address are values the element passes through to a nested
/// constructor, so they may sit where the crate asks for them.
const WRAPPED_HOLES: [&str; 3] = ["bus_device", "spi_device", "address"];

/// Check a fixed argument's holes, returning the names it carries.
///
/// An empty list is today's argument: a value the manifest already knows.
fn wrapped_argument(argument: &Expr) -> Result<Vec<String>, (String, String)> {
    let holes = sentinels(&quote::quote!(#argument).to_string());
    if holes.is_empty() {
        return Ok(holes);
    }
    if let Some(unknown) = holes
        .iter()
        .find(|hole| !WRAPPED_HOLES.contains(&hole.as_str()))
    {
        return Err((
            format!("`{{{unknown}}}` must be an argument of its own"),
            "only `{bus_device}`, `{spi_device}` and `{address}` may sit inside an expression".into(),
        ));
    }
    let buses = holes
        .iter()
        .filter(|hole| matches!(hole.as_str(), "bus_device" | "spi_device"))
        .count();
    if buses > 1 {
        return Err((
            "the constructor takes the bus twice".into(),
            "write `{bus_device}` once: one bus wrapper per argument".into(),
        ));
    }
    Ok(holes)
}

/// The `{name}` holes a substituted expression still carries, in order and
/// without repeats.
fn sentinels(tokens: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = tokens;
    while let Some(at) = rest.find("__espforge_") {
        rest = &rest[at + "__espforge_".len()..];
        let end = rest
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..end];
        if !name.is_empty() && !out.iter().any(|known| known == name) {
            out.push(name.to_string());
        }
        rest = &rest[end..];
    }
    out
}

/// A fixed argument's text with its holes spelled `{name}` again.
///
/// Longer names go first so `{bus_device_extra}` is not rewritten by the
/// `{bus_device}` replacement.
fn template_text(argument: &Expr, mut holes: Vec<String>) -> String {
    let mut text = quote::quote!(#argument).to_string();
    holes.sort_by_key(|hole| std::cmp::Reverse(hole.len()));
    for hole in holes {
        text = text.replace(&format!("__espforge_{hole}"), &format!("{{{hole}}}"));
    }
    text
}

/// A constructor argument reduced to what two constructors must agree on.
type ArgShape<'a> = (Option<&'a str>, Option<&'a str>);

struct ConstructorCtx<'a> {
    tag: &'a str,
    bus: &'a BusDecl,
    pins: &'a [PinDecl],
    attrs: &'a [AttrDef],
    maps: &'a BTreeMap<&'a str, &'a MapDecl>,
}

/// Parse a constructor call template and return its path and its placeholder
/// arguments, in order.
fn parse_constructor(
    text: &str,
    span: Range<usize>,
    ctx: &ConstructorCtx<'_>,
    errors: &mut Vec<SchemaError>,
) -> Option<ConstructorArgs> {
    // A busless constructor takes no bus and no address: its slots are the
    // declared pins, mapped attributes and fixed expressions.
    let busless = ctx.bus.kind == "none";
    let mut allowed: Vec<&str> = if busless {
        Vec::new()
    } else {
        vec!["bus_device", "cs_pin", "spi_device", "address"]
    };
    allowed.extend(ctx.attrs.iter().map(|attr| attr.name.as_str()));
    for pin in ctx.pins {
        allowed.push(pin.name.as_str());
        // `{dc_pin}` is the spelling the phase-8 format uses; `{dc}` is the
        // pin's own name. Both mean the same claimed line.
        allowed.push(pin_alias(pin.name.as_str()).leak());
    }
    validate_placeholders(text, &allowed, span.clone(), "constructor", errors);

    let substituted = match substitute(text) {
        Ok(text) => text,
        Err((message, label)) => {
            errors.push(err(span, message, label));
            return None;
        }
    };
    let expression: Expr = match syn::parse_str(&substituted) {
        Ok(expression) => expression,
        Err(error) => {
            errors.push(err(
                span,
                "the constructor is not a Rust expression",
                error.to_string(),
            ));
            return None;
        }
    };
    let Expr::Call(call) = &expression else {
        errors.push(err(
            span,
            "the constructor must be a call",
            "write `Type::new({bus_device}, ..)`",
        ));
        return None;
    };
    let Expr::Path(path) = &*call.func else {
        errors.push(err(
            span,
            "the constructor must name a path",
            "write `Type::new({bus_device}, ..)`",
        ));
        return None;
    };
    let Some(new) = path_text(&path.path) else {
        errors.push(err(
            span.clone(),
            "the constructor path must be plain identifiers",
            "write `crate::Type::new`",
        ));
        return None;
    };

    let mut args: Vec<CtorSpec> = Vec::new();
    for argument in &call.args {
        // A bare sentinel identifier is a hole the element fills; any other
        // expression is a value the manifest already knows.
        let placeholder = match argument {
            Expr::Path(path) => placeholder_of(&path.path),
            _ => None,
        };
        let Some(placeholder) = placeholder else {
            // A fixed argument may still hole out the bus and the address: a
            // driver that takes a wrapper (`I2CExpander::new({bus_device},
            // {address})`) is manifest-only too.
            match wrapped_argument(&argument) {
                Ok(holes) => args.push(CtorSpec::Fixed {
                    text: template_text(&argument, holes),
                    span: span.clone(),
                }),
                Err((message, label)) => errors.push(err(span.clone(), message, label)),
            }
            continue;
        };
        let resolved = resolve_pin(ctx.pins, &placeholder)
            .map(str::to_string)
            .unwrap_or_else(|| placeholder.clone());
        if args.iter().any(|known| known.uses(&resolved)) {
            errors.push(err(
                span.clone(),
                format!("`{{{placeholder}}}` appears twice in the constructor"),
                "each constructor argument is filled once",
            ));
        }
        if placeholder == "address"
            && !matches!(ctx.bus.address, AddressNeed::Any | AddressNeed::Fixed(_))
        {
            errors.push(err(
                span.clone(),
                format!(
                    "`{}` has no I2C address but its constructor takes `{{address}}`",
                    ctx.tag
                ),
                "add an `address` to `bus \"i2c\"` or drop the argument",
            ));
        }
        if placeholder == "cs_pin" && ctx.bus.kind != "spi" {
            errors.push(err(
                span.clone(),
                "`{cs_pin}` is an SPI chip-select argument",
                "use `{bus_device}` on an I2C bus",
            ));
        }
        if !ctx.maps.contains_key(resolved.as_str())
            && !ctx.pins.iter().any(|pin| pin.name == resolved)
            && !matches!(
                placeholder.as_str(),
                "bus_device" | "spi_device" | "cs_pin" | "address"
            )
            && ctx.attrs.iter().any(|attr| attr.name == resolved)
        {
            errors.push(err(
                span.clone(),
                format!("`{{{placeholder}}}` has no `map` in `codegen`"),
                format!("add `map \"{placeholder}\" {{ .. }}`"),
            ));
        }
        args.push(CtorSpec::Placeholder {
            name: resolved,
            span: span.clone(),
        });
    }
    // A busless constructor has no bus to consume, so every argument stays.
    if busless {
        return Some((new, args));
    }

    // A bus device consumes its endpoint exactly once, either as an argument
    // of its own (wherever the manifest put it) or inside one wrapper. A
    // constructor that never mentions it would leave the bus unused.
    let buses = args
        .iter()
        .filter(|spec| spec.uses("bus_device") || spec.uses("spi_device"))
        .count();
    if buses == 0 {
        errors.push(err(
            span,
            format!("`{}` constructor does not take the bus", ctx.tag),
            "write `{bus_device}` once, on its own or inside a wrapper",
        ));
    } else if buses > 1 {
        errors.push(err(
            span,
            "the constructor takes the bus twice",
            "write `{bus_device}` once: one bus argument or one wrapper",
        ));
    }
    Some((new, args))
}

fn placeholder_of(path: &syn::Path) -> Option<String> {
    if path.segments.len() != 1 {
        return None;
    }
    let ident = path.segments.first()?.ident.to_string();
    ident
        .strip_prefix("__espforge_")
        .map(|name| name.to_string())
}

fn path_text(path: &syn::Path) -> Option<String> {
    let mut out = String::new();
    for (index, segment) in path.segments.iter().enumerate() {
        if !matches!(segment.arguments, syn::PathArguments::None) {
            return None;
        }
        if index > 0 {
            out.push_str("::");
        }
        out.push_str(&segment.ident.to_string());
    }
    Some(out)
}

/// Replace every `{name}` hole with a sentinel identifier.
fn substitute(text: &str) -> Result<String, (String, String)> {
    let mut out = String::new();
    let mut rest = text;
    let mut in_string = false;
    let mut escaped = false;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if in_string {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                out.push(character);
            }
            '{' => {
                let Some((name, after)) = rest.split_once('}') else {
                    return Err((
                        "unclosed `{` in a codegen template".into(),
                        "write `{name}`".into(),
                    ));
                };
                let name = name.trim();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err((
                        format!("`{{{name}}}` is not a placeholder"),
                        "placeholder names are identifiers".into(),
                    ));
                }
                out.push_str("__espforge_");
                out.push_str(name);
                rest = after;
            }
            '}' => {
                return Err((
                    "unmatched `}` in a codegen template".into(),
                    "write `{name}`".into(),
                ));
            }
            _ => out.push(character),
        }
    }
    Ok(out)
}

fn validate_type(text: &str, span: Range<usize>, errors: &mut Vec<SchemaError>) {
    validate_type_in(text, "type", span, errors);
}

/// [`validate_type`] for a template that is not `type`: a v2 `endpoint` is the
/// same kind of text, but the diagnostic names the block it came from.
pub(crate) fn validate_type_in(
    text: &str,
    block: &str,
    span: Range<usize>,
    errors: &mut Vec<SchemaError>,
) {
    let Ok(substituted) = substitute(text) else {
        return;
    };
    if let Err(error) = syn::parse_str::<Type>(&substituted) {
        errors.push(err(
            span,
            format!("`{block}` is not a Rust type"),
            error.to_string(),
        ));
    }
}

/// The first `{name}` a template contains, if any. A `{`–`}` mismatch is left
/// to [`validate_placeholders`], which reports it.
pub(crate) fn first_placeholder(text: &str) -> Option<String> {
    let mut rest = text;
    let mut in_string = false;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if in_string {
            if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => {
                let (name, _) = rest.split_once('}')?;
                return Some(name.trim().to_string());
            }
            _ => {}
        }
    }
    None
}

fn validate_expr(text: &str, span: Range<usize>, errors: &mut Vec<SchemaError>) {
    let Ok(substituted) = substitute(text) else {
        return;
    };
    if let Err(error) = syn::parse_str::<Expr>(&substituted) {
        errors.push(err(
            span,
            "`expr` is not a Rust expression",
            error.to_string(),
        ));
    }
}

fn validate_placeholders(
    text: &str,
    allowed: &[&str],
    span: Range<usize>,
    block: &str,
    errors: &mut Vec<SchemaError>,
) {
    let mut rest = text;
    let mut in_string = false;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if in_string {
            if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => {
                let Some((name, after)) = rest.split_once('}') else {
                    errors.push(err(
                        span.clone(),
                        "unclosed `{` in a codegen template",
                        "write `{name}`",
                    ));
                    return;
                };
                let name = name.trim();
                if !allowed.contains(&name) {
                    errors.push(err(
                        span.clone(),
                        format!("`{{{name}}}` is not available in `{block}`"),
                        format!("known placeholders: {}", allowed.join(", ")),
                    ));
                }
                rest = after;
            }
            '}' => {
                errors.push(err(
                    span.clone(),
                    "unmatched `}` in a codegen template",
                    "write `{name}`",
                ));
                return;
            }
            _ => {}
        }
    }
}

/// The `{<name>_pin}` spelling for a pin called `name`.
fn pin_alias(name: &str) -> String {
    format!("{name}_pin")
}

fn resolve_pin<'a>(pins: &'a [PinDecl], placeholder: &str) -> Option<&'a str> {
    pins.iter()
        .map(|pin| pin.name.as_str())
        .find(|name| *name == placeholder || pin_alias(name) == placeholder)
}

/// The Rust expression one pin argument carries.
///
/// A line the app only writes is an `Output`; a line it reads — an interrupt,
/// a data-ready flag, an e-paper BUSY — is an `Input` with its pull; a line it
/// does both on — a one-wire data pin — is a `Bidirectional` (a `Flex` the
/// runtime type switches). The pin is moved out of the registry the same way in
/// every case: the resource graph has already claimed it, which is what makes
/// the `clone_unchecked` sound.
fn pin_template(direction: PinDirection, level: Level, pull: Pull) -> String {
    match direction {
        PinDirection::Output => format!(
            "esp_hal::gpio::Output::new(unsafe {{ registry.peripherals.GPIO{{n}}.clone_unchecked() }}, esp_hal::gpio::Level::{}, esp_hal::gpio::OutputConfig::default())",
            level.word()
        ),
        PinDirection::Input => format!(
            "esp_hal::gpio::Input::new(unsafe {{ registry.peripherals.GPIO{{n}}.clone_unchecked() }}, esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::{}))",
            pull.word()
        ),
        PinDirection::Bidirectional => format!(
            "espforge_runtime::Bidirectional::new(unsafe {{ registry.peripherals.GPIO{{n}}.clone_unchecked() }}, esp_hal::gpio::Pull::{})",
            pull.word()
        ),
        PinDirection::OpenDrain => format!(
            "espforge_runtime::OpenDrainPin::new(unsafe {{ registry.peripherals.GPIO{{n}}.clone_unchecked() }}, esp_hal::gpio::Level::{}, esp_hal::gpio::Pull::{})",
            level.word(),
            pull.word()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(text: &str) -> Result<Schema, Vec<SchemaError>> {
        load_all(&[SCHEMA_KDL, SERVICES_SCHEMA_KDL, text])
    }

    const DRIVER: &str = r##"
driver "demo" {
    format_version 2
    summary "Demo"
    category "sensor"
    dependency "embedded-graphics" { version "0.8.1" }
    bus "i2c" { address default="0x10" allowed="0x08..=0x77" }
    codegen {
        feature "demo"
        runtime "blocking" {
            type "demo::Demo"
            constructor "demo::Demo::new({bus_device}, {address})"
            endpoint "demo::Endpoint"
        }
    }
    attr "init" kind="choice" default="boot" { choices "boot" "deferred" }
    example "minimal" #"<demo id="d" bus="$i2c0" />"#
}
"##;

    /// A `bus "none"` driver: no bus peripheral, no `bus` attribute, and a
    /// constructor that takes only its own pins and mapped values.
    const BUSLESS: &str = r##"
driver "hx711" {
    format_version 2
    summary "HX711 load-cell ADC"
    category "sensor"
    dependency "loadcell" { version "0.2.0" }
    bus "none"
    pin "sck" direction="output" default_level="low"
    pin "dout" direction="input" pull="none"
    codegen {
        fallible #false
        map "delay" { "runtime" { expr "espforge_runtime::Delay::new()" } }
        runtime "blocking" {
            type "loadcell::hx711::HX711<espforge_runtime::OutputPin, espforge_runtime::InputPin, espforge_runtime::Delay>"
            constructor "loadcell::hx711::HX711::new({sck}, {dout}, {delay})"
        }
    }
    attr "delay" kind="choice" default="runtime" { choices "runtime" }
    example "minimal" #"<hx711 id="scale" sck="4" dout="5" />"#
}
"##;

    /// A dual-bus part declares one `transport` block per bus kind. The
    /// element accepts the union of their bus attributes, each optional, and
    /// keeps one recipe per kind.
    const MULTI: &str = r##"
driver "dual" {
    format_version 2
    summary "Dual-bus sensor"
    category "sensor"
    dependency "dual" { version "1.0.0" }

    transport "i2c" {
        bus { address default="0x76" }
        codegen {
            fallible #false
            runtime "blocking" {
                type "dual::i2c::Dual"
                constructor "dual::i2c::Dual::new({bus_device}, {address})"
            }
        }
        dependency "dual" { features "i2c" }
    }
    transport "spi" {
        bus { mode 3 frequency "10000kHz" }
        codegen {
            fallible #false
            runtime "blocking" {
                type "dual::spi::Dual"
                constructor "dual::spi::Dual::new({spi_device})"
            }
        }
        dependency "dual" { features "spi" }
    }

    example "i2c" #"<dual id="d" bus="$i2c0" address="0x76" />"#
    example "spi" #"<dual id="d" bus="$spi2" cs="10" />"#
}
"##;

    #[test]
    fn a_multi_transport_driver_loads_one_recipe_per_kind() {
        let schema = load(MULTI).expect("the driver loads");
        let element = schema.element("dual").expect("tag");
        let recipe = element.device_recipe.as_ref().expect("a recipe");
        assert_eq!(recipe.bus, "i2c", "the first transport is the element's own");
        assert_eq!(recipe.transports.len(), 2);
        assert_eq!(recipe.transports[0].bus, "i2c");
        assert_eq!(recipe.transports[1].bus, "spi");
        assert_eq!(recipe.transports[1].ty, "dual::spi::Dual");
        assert!(recipe.transports[1].spi_config);

        // The union of bus attributes is optional: which one is required is
        // decided by the bus the spec binds to, at lowering time.
        let bus = element
            .attrs
            .iter()
            .find(|attr| attr.name == "bus")
            .expect("the bus attribute");
        assert!(bus.required);
        assert!(matches!(
            &bus.kind,
            AttrKind::Ref { to } if to.as_slice() == ["i2c", "spi"]
        ));
        for name in ["address", "cs", "mode", "frequency"] {
            let attr = element
                .attrs
                .iter()
                .find(|attr| attr.name == name)
                .unwrap_or_else(|| panic!("`{name}` is in the union"));
            assert!(!attr.required, "`{name}` is optional on the element");
        }
        let mode = element
            .attrs
            .iter()
            .find(|attr| attr.name == "mode")
            .expect("mode is in the union");
        assert_eq!(mode.default.as_deref(), Some("3"));

        // Only the selected transport's Cargo features reach the project.
        assert_eq!(recipe.transports[0].dependencies[0].features, ["i2c"]);
        assert_eq!(recipe.transports[1].dependencies[0].features, ["spi"]);
    }

    #[test]
    fn a_multi_transport_driver_keeps_its_bus_and_codegen_in_the_transports() {
        let bad = MULTI.replace(
            "    transport \"i2c\" {",
            "    bus \"i2c\" {}\n    codegen {}\n    transport \"i2c\" {",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("both `transport`")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_single_transport_block_is_not_multi_transport() {
        let text = r##"
driver "dual" {
    format_version 2
    summary "One bus"
    dependency "dual" { version "1.0.0" }
    transport "i2c" {
        bus { address default="0x76" }
        codegen {
            fallible #false
            runtime "blocking" {
                type "dual::i2c::Dual"
                constructor "dual::i2c::Dual::new({bus_device}, {address})"
            }
        }
    }
    example "i2c" #"<dual id="d" bus="$i2c0" />"#
}
"##;
        let errors = load(text).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("only one `transport`")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_transport_kind_must_match_its_bus() {
        let bad = MULTI.replace(
            "    transport \"spi\" {\n        bus { mode 3 frequency \"10000kHz\" }",
            "    transport \"uart\" {\n        bus \"spi\" { mode 3 frequency \"10000kHz\" }",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("declares a `spi` bus")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_duplicate_transport_kind_is_refused() {
        let bad = MULTI
            .replace("    transport \"spi\" {", "    transport \"i2c\" {")
            .replace(
                "bus { mode 3 frequency \"10000kHz\" }",
                "bus { address default=\"0x77\" }",
            );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "duplicate `transport` `i2c`"),
            "{errors:?}"
        );
    }

    #[test]
    fn a_busless_driver_loads_without_a_bus_attribute() {
        let schema = load(BUSLESS).expect("the driver loads");
        let element = schema.element("hx711").expect("tag");
        let recipe = element.device_recipe.as_ref().expect("a recipe");
        assert_eq!(recipe.bus, "none");
        assert_eq!(recipe.new, "loadcell::hx711::HX711::new");
        assert_eq!(recipe.args.len(), 3, "two pins and the delay map");
        assert!(
            !element.attrs.iter().any(|attr| attr.name == "bus"),
            "a busless driver declares no `bus` attribute"
        );
        let sck = element
            .attrs
            .iter()
            .find(|attr| attr.name == "sck")
            .expect("the pin is declared");
        assert!(
            matches!(sck.kind, AttrKind::PinOrRef),
            "a busless driver names its lines by number"
        );
        assert!(element.attrs.iter().any(|attr| attr.name == "delay"));
    }

    #[test]
    fn an_open_drain_pin_is_a_bidirectional_claim() {
        let text = BUSLESS
            .replace(
                "direction=\"output\" default_level=\"low\"",
                "direction=\"open_drain\" default_level=\"high\"",
            )
            .replace(
                "direction=\"input\" pull=\"none\"",
                "direction=\"open_drain\" default_level=\"high\"",
            );
        let schema = load(&text).expect("open-drain pins load");
        let recipe = schema
            .element("hx711")
            .expect("tag")
            .device_recipe
            .clone()
            .expect("a recipe");
        let drains: Vec<_> = recipe
            .args
            .iter()
            .filter_map(|arg| arg.slot())
            .filter(|arg| arg.direction == PinDirection::OpenDrain)
            .collect();
        assert_eq!(drains.len(), 2);
        assert!(drains.iter().all(|arg| arg.open_drain));
    }

    #[test]
    fn a_busless_driver_cannot_take_a_bus() {
        let text = BUSLESS.replace(
            "HX711::new({sck}, {dout}, {delay})",
            "HX711::new({bus_device}, {sck}, {dout}, {delay})",
        );
        let errors = load(&text).unwrap_err();
        assert!(
            format!("{errors:?}").contains("bus_device"),
            "{errors:?}"
        );
    }

    #[test]
    fn a_busless_driver_needs_a_pin() {
        let text = BUSLESS
            .replace(
                "    pin \"sck\" direction=\"output\" default_level=\"low\"\n",
                "",
            )
            .replace("    pin \"dout\" direction=\"input\" pull=\"none\"\n", "")
            .replace("HX711::new({sck}, {dout}, {delay})", "HX711::new({delay})");
        let errors = load(&text).unwrap_err();
        assert!(
            format!("{errors:?}").contains("declares no pin"),
            "{errors:?}"
        );
    }

    #[test]
    fn a_v2_driver_loads_into_the_device_model() {
        let schema = load(DRIVER).expect("the driver loads");
        let element = schema.element("demo").expect("tag");
        assert_eq!(element.format, Format::V2);
        assert_eq!(element.section, "devices");
        let recipe = element.device_recipe.as_ref().unwrap();
        assert_eq!(recipe.bus, "i2c");
        assert_eq!(recipe.feature, "demo");
        assert_eq!(recipe.address, AddressNeed::Any);
        assert_eq!(recipe.address_default, Some(0x10));
        assert_eq!(recipe.address_allowed, Some((0x08, 0x77)));
        assert_eq!(recipe.new, "demo::Demo::new");
        let address = element
            .attrs
            .iter()
            .find(|attr| attr.name == "address")
            .unwrap();
        assert!(!address.required);
        assert_eq!(address.default.as_deref(), Some("0x10"));
    }

    /// A crate that puts its blocking and async APIs behind one feature each
    /// says so in the dependency, not in the manifest's prose.
    #[test]
    fn a_dependency_declares_which_half_each_mode_needs() {
        let text = DRIVER.replace(
            "dependency \"embedded-graphics\" { version \"0.8.1\" }",
            "dependency \"mode-crate\" {\n        version \"1.0.0\"\n        features \"blocking\"\n        \
             async_features \"async\"\n        default_features #false\n        async_default_features \
             #true\n        modes_are_exclusive #true\n    }",
        );
        let schema = load(&text).expect("the dependency loads");
        let dependency = &schema.element("demo").expect("tag").dependencies[0];
        assert_eq!(dependency.features, ["blocking"]);
        assert_eq!(dependency.async_features, ["async"]);
        assert_eq!(dependency.async_default_features, Some(true));
        assert!(!dependency.default_features);
        assert!(dependency.modes_are_exclusive);
    }

    /// An absent `async_default_features` keeps the blocking value; a crate
    /// whose default features stay on in both modes says nothing extra.
    #[test]
    fn a_dependency_without_per_mode_defaults_inherits_them() {
        let text = DRIVER.replace(
            "dependency \"embedded-graphics\" { version \"0.8.1\" }",
            "dependency \"mode-crate\" { version \"1.0.0\" }",
        );
        let schema = load(&text).expect("the dependency loads");
        let dependency = &schema.element("demo").expect("tag").dependencies[0];
        assert_eq!(dependency.async_default_features, None);
        assert!(dependency.default_features);
        assert!(!dependency.modes_are_exclusive);
    }

    #[test]
    fn a_malformed_type_fails_at_load() {
        let bad = DRIVER.replace("type \"demo::Demo\"", "type \"demo::Demo<\"");
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("not a Rust type")),
            "{errors:?}"
        );
    }

    /// The endpoint is the type the bus helper already returns, so a generic
    /// type is the normal shape and must reach the recipe verbatim.
    #[test]
    fn an_endpoint_may_be_a_generic_type() {
        let generic = DRIVER.replace(
            "endpoint \"demo::Endpoint\"",
            "endpoint \"CriticalSectionDevice<'static, I2c<'static, Blocking>>\"",
        );
        let schema = load(&generic).expect("a generic endpoint loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(
            recipe.endpoint.as_deref(),
            Some("CriticalSectionDevice<'static, I2c<'static, Blocking>>")
        );
    }

    #[test]
    fn an_endpoint_that_is_not_a_type_is_refused() {
        let bad = DRIVER.replace("endpoint \"demo::Endpoint\"", "endpoint \"0x44\"");
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`endpoint` is not a Rust type"),
            "{errors:?}"
        );
    }

    /// An endpoint is a type, not a template: `{bus}` has nothing to fill it.
    #[test]
    fn an_endpoint_takes_no_placeholder() {
        let bad = DRIVER.replace(
            "endpoint \"demo::Endpoint\"",
            "endpoint \"Endpoint<{bus}>\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`endpoint` takes no `{placeholder}`"),
            "{errors:?}"
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("not a Rust type")),
            "the placeholder is reported as such, not as a `syn` failure: {errors:?}"
        );
    }

    #[test]
    fn an_infallible_constructor_is_not_mapped() {
        let infallible = DRIVER.replace("        feature \"demo\"\n", "        fallible #false\n");
        let schema = load(&infallible).expect("an infallible constructor loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert!(!recipe.fallible);
        assert_eq!(recipe.feature, "");
    }

    /// Only a crate the manifest names can return the device directly: the
    /// bundled API returns `DeviceErrorKind`.
    #[test]
    fn an_infallible_bundled_driver_is_refused() {
        let bad = DRIVER.replace(
            "        feature \"demo\"\n",
            "        feature \"demo\"\n        fallible #false\n",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error.message
                == "`fallible #false` needs a crate that returns the device directly"),
            "{errors:?}"
        );
    }

    /// A part at one `fixed` address may hard-code it in the crate: the
    /// manifest leaves `{address}` out and emission passes none.
    /// A constructor argument the manifest already knows is a fixed
    /// expression: it is not a slot, and it keeps the place it was written in.
    #[test]
    fn a_fixed_argument_keeps_its_place() {
        let fixed = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device}, {address}, demo::Config::default(), 20)\"",
        );
        let schema = load(&fixed).expect("fixed arguments load");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.args[0], crate::CtorArg::Bus);
        assert_eq!(recipe.args[1], crate::CtorArg::Address);
        assert_eq!(recipe.args.len(), 4);
        let text = |arg: &crate::CtorArg| match arg {
            crate::CtorArg::Fixed { text, .. } => text.clone(),
            other => panic!("a fixed argument, not {other:?}"),
        };
        assert!(text(&recipe.args[2]).contains("Config"));
        assert_eq!(text(&recipe.args[3]), "20");
    }

    /// A driver that takes a wrapper around the bus is still manifest-only: the
    /// bus and the address may sit inside the first argument.
    #[test]
    fn a_wrapper_argument_takes_the_bus_and_the_address() {
        let wrapped = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new(expander::Expander::new({bus_device}, {address}), 20)\"",
        );
        let schema = load(&wrapped).expect("a wrapped constructor loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.new, "demo::Demo::new");
        assert_eq!(recipe.args.len(), 2);
        let text = |arg: &crate::CtorArg| match arg {
            crate::CtorArg::Fixed { text, .. } => text.clone(),
            other => panic!("a fixed argument, not {other:?}"),
        };
        let expander = text(&recipe.args[0]);
        assert!(expander.contains("{bus_device}"), "{expander}");
        assert!(expander.contains("{address}"), "{expander}");
        assert_eq!(text(&recipe.args[1]), "20");
    }

    /// Every other slot is a whole argument: a probe line is claimed on its own
    /// and a mapped value is resolved by the position it sits in.
    #[test]
    fn a_slot_must_be_an_argument_of_its_own() {
        let bad = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device}, demo::wrap({init}))\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`{init}` must be an argument of its own"),
            "{errors:?}"
        );
    }

    /// An address wrapper is an argument of its own: a crate that takes
    /// `I2cAddr` needs no glue crate either.
    #[test]
    fn an_address_wrapper_sits_after_the_bus() {
        let wrapped = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device}, demo::Address::from({address}), 20)\"",
        );
        let schema = load(&wrapped).expect("an address wrapper loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.new, "demo::Demo::new");
        assert_eq!(recipe.args[0], crate::CtorArg::Bus);
        assert_eq!(recipe.args.len(), 3);
        let text = |arg: &crate::CtorArg| match arg {
            crate::CtorArg::Fixed { text, .. } => text.clone(),
            other => panic!("a fixed argument, not {other:?}"),
        };
        let wrapper = text(&recipe.args[1]);
        assert!(wrapper.contains("Address"), "{wrapper}");
        assert!(wrapper.contains("{address}"), "{wrapper}");
        assert_eq!(text(&recipe.args[2]), "20");
    }

    /// A wrapper may follow another argument: the crate decides where its
    /// address type goes, not the manifest.
    #[test]
    fn an_address_wrapper_may_follow_another_argument() {
        let wrapped = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device}, 20, demo::Address::from({address}))\"",
        );
        let schema = load(&wrapped).expect("an out-of-order wrapper loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.args[0], crate::CtorArg::Bus);
        assert_eq!(recipe.args.len(), 3);
        let text = |arg: &crate::CtorArg| match arg {
            crate::CtorArg::Fixed { text, .. } => text.clone(),
            other => panic!("a fixed argument, not {other:?}"),
        };
        assert_eq!(text(&recipe.args[1]), "20");
        assert!(text(&recipe.args[2]).contains("Address"));
    }

    /// A crate that takes the bus after another argument (`Sen6x::new(&mut
    /// timer, i2c)`) loads with the endpoint where the manifest put it.
    #[test]
    fn a_bus_may_come_after_another_argument() {
        let reordered = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new(demo::Config::default(), {bus_device}, {address})\"",
        );
        let schema = load(&reordered).expect("a reordered constructor loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert!(
            matches!(&recipe.args[0], crate::CtorArg::Fixed { text, .. } if text.contains("Config"))
        );
        assert_eq!(recipe.args[1], crate::CtorArg::Bus);
        assert_eq!(recipe.args[2], crate::CtorArg::Address);
    }

    /// The bus still has to reach the constructor, and only once.
    #[test]
    fn a_bus_device_must_take_the_bus_once() {
        let missing = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({address})\"",
        );
        let errors = load(&missing).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("does not take the bus")),
            "{errors:?}"
        );

        let twice = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device}, {bus_device}, {address})\"",
        );
        let errors = load(&twice).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "the constructor takes the bus twice"),
            "{errors:?}"
        );
    }

    /// `map "address"` turns every address the driver allows into the crate's
    /// own value, so a `SlaveAddr` newtype still comes from the manifest.
    #[test]
    fn a_mapped_address_loads() {
        let mapped = DRIVER
            .replace(
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x08..=0x77\" }",
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x10..=0x11\" }",
            )
            .replace(
                "        feature \"demo\"\n",
                "        feature \"demo\"\n        map \"address\" {\n            \"0x10\" { expr \"demo::Address::First\" }\n            \"0x11\" { expr \"demo::Address::Second\" }\n        }\n",
            );
        let schema = load(&mapped).expect("a mapped address loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(
            recipe.address_map,
            Some(vec![
                (0x10, "demo::Address::First".to_string()),
                (0x11, "demo::Address::Second".to_string()),
            ])
        );
    }

    /// The map must cover exactly what the bus allows, or lowering could meet
    /// an address it has no value for.
    #[test]
    fn a_mapped_address_needs_a_complete_range() {
        let bad = DRIVER
            .replace(
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x08..=0x77\" }",
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x10..=0x11\" }",
            )
            .replace(
                "        feature \"demo\"\n",
                "        feature \"demo\"\n        map \"address\" { \"0x10\" { expr \"demo::Address::First\" } }\n",
            );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`map \"address\"` has no entry for `0x11`"),
            "{errors:?}"
        );
    }

    /// Without an allowed range the address set is all of `0x08..=0x77`, so a
    /// map would silently cover nothing.
    #[test]
    fn a_mapped_address_needs_a_finite_range() {
        let bad = DRIVER
            .replace(
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x08..=0x77\" }",
                "bus \"i2c\" { address default=\"0x10\" }",
            )
            .replace(
                "        feature \"demo\"\n",
                "        feature \"demo\"\n        map \"address\" { \"0x10\" { expr \"demo::Address::First\" } }\n",
            );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`map \"address\"` in `demo` has no address range"),
            "{errors:?}"
        );
    }

    /// A map nothing reads is a mistake, even on a part at a fixed address.
    #[test]
    fn a_mapped_address_must_reach_the_constructor() {
        let bad = DRIVER
            .replace(
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x08..=0x77\" }",
                "bus \"i2c\" { address fixed=\"0x10\" }",
            )
            .replace(
                "constructor \"demo::Demo::new({bus_device}, {address})\"",
                "constructor \"demo::Demo::new({bus_device})\"",
            )
            .replace(
                "        feature \"demo\"\n",
                "        feature \"demo\"\n        map \"address\" { \"0x10\" { expr \"demo::Address::First\" } }\n",
            );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`map \"address\"` in `demo` is not used"),
            "{errors:?}"
        );
    }

    /// A constructor plus the calls the device needs after it: the manifest
    /// names the order of calls, and nothing else.
    #[test]
    fn a_then_step_runs_after_the_constructor() {
        let text = DRIVER.replace(
            "            endpoint \"demo::Endpoint\"\n",
            "            endpoint \"demo::Endpoint\"\n            then { call \"set_address({address})\" }\n",
        );
        let schema = load(&text).expect("a step loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.steps.len(), 1);
        assert_eq!(recipe.steps[0].text, "set_address({address})");
        assert!(!recipe.steps[0].awaits, "a blocking step is a plain call");
        assert!(
            !recipe.steps[0].rebind,
            "a step keeps the device unless it says otherwise"
        );
        assert!(recipe.steps[0].fallible, "a step maps its error by default");
        let call = recipe.steps[0].call().expect("a parsed call");
        assert!(call.text().contains("__espforge_receiver"), "{call:?}");
        assert!(call.text().contains("__espforge_address"), "{call:?}");
        assert!(recipe.async_mode.steps.is_empty());
    }

    /// A step says how its result is used: `rebind #true` keeps the value a
    /// typestate `init(self)` returned, and `fallible #false` names a setup
    /// call that cannot fail.
    #[test]
    fn a_then_step_says_whether_it_rebinds_and_is_fallible() {
        let text = DRIVER.replace(
            "            endpoint \"demo::Endpoint\"\n",
            "            endpoint \"demo::Endpoint\"\n            then {\n                call \"init()\"\n                rebind #true\n                fallible #false\n            }\n",
        );
        let schema = load(&text).expect("a step loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.steps.len(), 1);
        assert!(recipe.steps[0].rebind);
        assert!(!recipe.steps[0].fallible);
    }

    /// A step reads only what the constructor already takes: a value it cannot
    /// reach is a manifest error, not a `rustc` one.
    #[test]
    fn a_then_step_names_only_constructor_values() {
        let bad = DRIVER.replace(
            "            endpoint \"demo::Endpoint\"\n",
            "            endpoint \"demo::Endpoint\"\n            then { call \"init({init})\" }\n",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error.message
                == "`{init}` in a `then` step is not a constructor argument"),
            "{errors:?}"
        );
    }

    /// A step takes no receiver text: the generated code calls it on the value
    /// it just built, which is what keeps a manifest from reaching elsewhere.
    #[test]
    fn a_then_step_takes_no_receiver() {
        let bad = DRIVER.replace(
            "            endpoint \"demo::Endpoint\"\n",
            "            endpoint \"demo::Endpoint\"\n            then { call \"self.init()\" }\n",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "a `then` step takes no receiver"),
            "{errors:?}"
        );
    }

    /// `awaits` is the async runtime's: a blocking step is a plain call.
    #[test]
    fn awaits_belongs_to_the_async_runtime() {
        let bad = DRIVER.replace(
            "            endpoint \"demo::Endpoint\"\n",
            "            endpoint \"demo::Endpoint\"\n            then {\n                call \"init()\"\n                awaits #true\n            }\n",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`awaits` belongs to the async runtime"),
            "{errors:?}"
        );
    }

    /// Steps are per runtime: the async half may call a synchronous `init`
    /// even though its constructor is awaited.
    #[test]
    fn an_async_step_says_when_it_is_not_awaited() {
        let dual = DRIVER.replace(
            "        runtime \"blocking\" {\n            type \"demo::Demo\"\n            constructor \"demo::Demo::new({bus_device}, {address})\"\n            endpoint \"demo::Endpoint\"\n        }\n",
            "        runtime \"blocking\" {\n            type \"demo::Demo\"\n            constructor \"demo::Demo::new({bus_device}, {address})\"\n            endpoint \"demo::Endpoint\"\n        }\n        runtime \"async\" {\n            type \"demo::Demo\"\n            constructor \"demo::Demo::new({bus_device}, {address})\"\n            endpoint \"demo::Endpoint\"\n            then {\n                call \"init()\"\n                awaits #false\n            }\n        }\n",
        );
        let schema = load(&dual).expect("a dual-mode driver loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert!(recipe.steps.is_empty());
        assert_eq!(recipe.async_mode.steps.len(), 1);
        assert!(!recipe.async_mode.steps[0].awaits);
    }

    /// Two constructors of one driver take the same things: a dual-mode
    /// manifest whose halves pass different constants is a bug.
    #[test]
    fn dual_mode_constructors_must_take_the_same_constants() {
        let dual = DRIVER.replace(
            "        runtime \"blocking\" {\n            type \"demo::Demo\"\n            constructor \"demo::Demo::new({bus_device}, {address})\"\n            endpoint \"demo::Endpoint\"\n        }\n",
            "        runtime \"blocking\" {\n            type \"demo::Demo\"\n            constructor \"demo::Demo::new({bus_device}, {address}, 20)\"\n            endpoint \"demo::Endpoint\"\n        }\n        runtime \"async\" {\n            type \"demo::Demo\"\n            constructor \"demo::Demo::new({bus_device}, {address}, 40)\"\n            endpoint \"demo::Endpoint\"\n        }\n",
        );
        let errors = load(&dual).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .message
                == "the async constructor of `demo` takes different arguments"),
            "{errors:?}"
        );
    }

    /// A fixed argument is still Rust: a malformed one is a load error, not a
    /// `rustc` error in the generated project.
    #[test]
    fn a_fixed_argument_must_be_a_rust_expression() {
        let bad = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device}, {address}, 2 +)\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "the constructor is not a Rust expression"),
            "{errors:?}"
        );
    }

    #[test]
    fn a_fixed_address_need_not_reach_the_constructor() {
        let fixed = DRIVER
            .replace(
                "bus \"i2c\" { address default=\"0x10\" allowed=\"0x08..=0x77\" }",
                "bus \"i2c\" { address fixed=\"0x10\" }",
            )
            .replace(
                "constructor \"demo::Demo::new({bus_device}, {address})\"",
                "constructor \"demo::Demo::new({bus_device})\"",
            );
        let schema = load(&fixed).expect("a fixed-address driver loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert!(!recipe.args.contains(&crate::CtorArg::Address));
        assert_eq!(recipe.address, AddressNeed::Fixed(0x10));
    }

    /// A project-chosen address must still reach the constructor.
    #[test]
    fn an_any_address_must_reach_the_constructor() {
        let bad = DRIVER.replace(
            "constructor \"demo::Demo::new({bus_device}, {address})\"",
            "constructor \"demo::Demo::new({bus_device})\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .message
                == "`demo` declares an address but its constructor does not take it"),
            "{errors:?}"
        );
    }

    #[test]
    fn fallible_defaults_to_true() {
        let schema = load(DRIVER).expect("the driver loads");
        let recipe = schema
            .element("demo")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert!(recipe.fallible);
    }

    #[test]
    fn an_unknown_placeholder_fails_at_load() {
        let bad = DRIVER.replace("{address}", "{nope}");
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error.message.contains("{nope}")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_placeholder_in_a_string_literal_is_refused() {
        let bad = DRIVER.replace("type \"demo::Demo\"", "type \"demo::Demo<{size}>\"");
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("not available")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_dependency_must_name_one_source_and_a_git_needs_a_rev() {
        let bad = DRIVER.replace(
            "dependency \"embedded-graphics\" { version \"0.8.1\" }",
            "dependency \"embedded-graphics\" {\n            version \"0.8.1\"\n            path \"../x\"\n            git \"https://x\"\n        }",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error.message.contains("sources")),
            "{errors:?}"
        );
    }

    #[test]
    fn fixed_and_default_addresses_are_mutually_exclusive() {
        let bad = DRIVER.replace(
            "address default=\"0x10\" allowed=\"0x08..=0x77\"",
            "address fixed=\"0x10\" default=\"0x11\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("both `fixed` and `default`")),
            "{errors:?}"
        );
    }

    /// A driver with one output line and one input line.
    const PINNED: &str = r##"
driver "pinned" {
    format_version 2
    summary "Pinned"
    category "sensor"
    dependency "embedded-graphics" { version "0.8.1" }
    bus "i2c" { address fixed="0x10" }
    pin "dc" direction="output" default_level="low"
    pin "ready" direction="input" pull="up"
    codegen {
        feature "pinned"
        runtime "blocking" {
            type "pinned::Pinned"
            constructor "pinned::Pinned::new({bus_device}, {address}, {dc_pin}, {ready})"
        }
    }
    example "minimal" #"<pinned id="p" bus="$i2c0" address="0x10" dc="$gpio4" ready="$gpio5" />"#
}
"##;

    fn arg<'a>(recipe: &'a DeviceRecipe, name: &str) -> &'a DeviceArg {
        recipe
            .args
            .iter()
            .filter_map(|arg| arg.slot())
            .find(|arg| arg.name == name)
            .unwrap_or_else(|| panic!("`{name}` is an argument"))
    }

    #[test]
    fn an_input_pin_is_wrapped_as_an_input_with_its_pull() {
        let schema = load(PINNED).expect("the driver loads");
        let recipe = schema
            .element("pinned")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();

        let ready = arg(recipe, "ready");
        assert!(matches!(ready.direction, PinDirection::Input));
        let template = ready.pin.as_deref().expect("a pin slot");
        assert!(template.contains("esp_hal::gpio::Input::new"), "{template}");
        assert!(
            template.contains("InputConfig::default().with_pull(esp_hal::gpio::Pull::Up)"),
            "{template}"
        );

        // The output line keeps its level and is still claimed as an output.
        let dc = arg(recipe, "dc");
        assert!(matches!(dc.direction, PinDirection::Output));
        assert!(
            dc.pin
                .as_deref()
                .unwrap()
                .contains("esp_hal::gpio::Level::Low")
        );
    }

    /// A bidirectional line is the flex wrapper: it reads through the
    /// manifest's pull and starts released, so no level is rendered.
    #[test]
    fn a_bidirectional_pin_is_a_flex_line_with_its_pull() {
        let source = PINNED
            .replace(
                "pin \"ready\" direction=\"input\" pull=\"up\"",
                "pin \"line\" direction=\"bidirectional\" pull=\"up\"",
            )
            .replace("{ready}", "{line}")
            .replace("ready=\"$gpio5\"", "line=\"$gpio5\"");
        let schema = load(&source).expect("the driver loads");
        let recipe = schema
            .element("pinned")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();

        let line = arg(recipe, "line");
        assert!(matches!(line.direction, PinDirection::Bidirectional));
        let template = line.pin.as_deref().expect("a pin slot");
        assert!(
            template.contains("espforge_runtime::Bidirectional::new"),
            "{template}"
        );
        assert!(template.contains("esp_hal::gpio::Pull::Up"), "{template}");
        assert!(!template.contains("Level::"), "{template}");
    }

    #[test]
    fn a_bidirectional_line_cannot_declare_a_level() {
        let bad = PINNED.replace(
            "pin \"ready\" direction=\"input\" pull=\"up\"",
            "pin \"ready\" direction=\"bidirectional\" default_level=\"high\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .message
                .contains("`default_level` is for a driven pin")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_sideways_direction_is_refused() {
        let bad = PINNED.replace("direction=\"input\"", "direction=\"sideways\"");
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`sideways` is not a pin direction"),
            "{errors:?}"
        );
    }

    #[test]
    fn a_pull_needs_an_input_and_a_level_needs_an_output() {
        let bad = PINNED.replace(
            "direction=\"output\" default_level=\"low\"",
            "direction=\"output\" default_level=\"low\" pull=\"up\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .message
                .contains("`pull` is for an input or bidirectional line")),
            "{errors:?}"
        );

        let bad = PINNED.replace(
            "direction=\"input\" pull=\"up\"",
            "direction=\"input\" pull=\"up\" default_level=\"high\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .message
                .contains("`default_level` is for a driven pin")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_pin_colliding_with_an_attr_is_refused() {
        let bad = DRIVER.replace(
            "    attr \"init\"",
            "    pin \"dc\"\n    attr \"dc\" kind=\"bool\" default=\"false\"\n    attr \"init\"",
        );
        let errors = load(&bad).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("duplicate attribute")),
            "{errors:?}"
        );
    }

    /// A companion chip: SPI, a chip select, an input line and a choice.
    const PART: &str = r##"
driver "part" {
    format_version 2
    summary "Companion part"
    category "sensor"
    dependency "embedded-graphics" { version "0.8.1" }
    bus "spi" { }
    pin "irq" direction="input" pull="up"
    codegen {
        feature "part"
        runtime "blocking" {
            type "part::Part"
            constructor "part::Part::new({bus_device}, {cs_pin}, {irq}, {init})"
        }
        map "init" {
            "boot" { expr "part::Init::Boot" }
            "deferred" { expr "part::Init::Deferred" }
        }
    }
    attr "init" kind="choice" default="boot" { choices "boot" "deferred" }
    example "minimal" #"<part id="p" bus="$spi2" cs="9" irq="$gpio5" />"#
}
"##;

    const COMPOSITE: &str = r##"
driver "panel" {
    format_version 2
    summary "Panel with a companion"
    category "display"
    dependency "embedded-graphics" { version "0.8.1" }
    bus "spi" { }
    pin "dc" direction="output"
    pin "touch_cs" direction="output"
    pin "touch_irq" direction="input" pull="up"
    compose "part" {
        attr "cs" from="touch_cs"
        attr "irq" from="touch_irq"
        attr "init" value="deferred"
    }
    codegen {
        feature "panel"
        runtime "blocking" {
            type "panel::Panel"
            constructor "panel::Panel::new({bus_device}, {cs_pin}, {dc})"
        }
    }
    example "minimal" #"<panel id="p" bus="$spi2" cs="9" dc="$gpio4" />"#
}
"##;

    #[test]
    fn a_driver_declares_a_composed_part() {
        let schema = load(&format!("{PART}{COMPOSITE}")).expect("the pair loads");
        let recipe = schema
            .element("panel")
            .expect("tag")
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.parts.len(), 1);
        let part = &recipe.parts[0];
        assert_eq!(part.tag, "part");
        let mapped = |name: &str| {
            part.attrs
                .iter()
                .find(|attr| attr.name == name)
                .map(|attr| attr.source.clone())
        };
        assert_eq!(mapped("cs"), Some(PartSource::From("touch_cs".into())));
        assert_eq!(mapped("irq"), Some(PartSource::From("touch_irq".into())));
        assert_eq!(mapped("init"), Some(PartSource::Value("deferred".into())));

        // The mapped lines are the composite's own attributes, so the spec
        // writes `touch_cs=` and the part never appears in it.
        let panel = schema.element("panel").unwrap();
        for name in ["touch_cs", "touch_irq"] {
            assert!(
                panel.attrs.iter().any(|attr| attr.name == name),
                "{name} is declared on the composite"
            );
        }
        // Every bundled driver is one part.
        assert!(
            schema
                .elements
                .iter()
                .filter(|element| element.device_recipe.is_some())
                .all(|element| element.tag == "panel"
                    || element.device_recipe.as_ref().unwrap().parts.is_empty())
        );
    }

    #[test]
    fn a_composition_needs_a_known_driver_on_the_same_bus() {
        let unknown = COMPOSITE.replace("compose \"part\"", "compose \"nope\"");
        let errors = load(&format!("{PART}{unknown}")).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("unknown driver `nope`")),
            "{errors:?}"
        );

        // `<led>` is a pin device, not a driver.
        let not_a_driver = COMPOSITE.replace("compose \"part\"", "compose \"led\"");
        let errors = load(&format!("{PART}{not_a_driver}")).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("`<led>` is not a driver")),
            "{errors:?}"
        );

        // `demo` is an I2C driver and `panel` is an SPI composite.
        let other_bus = COMPOSITE.replace("compose \"part\"", "compose \"demo\"");
        let errors = load(&format!("{PART}{DRIVER}{other_bus}")).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("drives an <i2c> bus")),
            "{errors:?}"
        );
    }

    /// A driver that composes another driver, which composes another.
    fn layered(driver: &str, part: &str) -> String {
        format!(
            r##"
driver "{driver}" {{
    format_version 2
    summary "Layered"
    category "sensor"
    dependency "embedded-graphics" {{ version "0.8.1" }}
    bus "i2c" {{ address default="0x10" }}
    compose "{part}" {{ }}
    codegen {{
        feature "{driver}"
        runtime "blocking" {{
            type "{driver}::{driver}"
            constructor "{driver}::{driver}::new({{bus_device}}, {{address}})"
        }}
    }}
    example "minimal" #"<{driver} id="d" bus="$i2c0" />"#
}}
"##
        )
    }

    #[test]
    fn a_composed_driver_cannot_compose_again() {
        let source = format!(
            "{}{}{}",
            layered("inner", "demo"),
            layered("middle", "inner"),
            layered("outer", "middle")
        );
        let errors = load(&source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("composes `<inner>` too")),
            "{errors:?}"
        );
    }

    /// A `value=` literal stands where a spec attribute would, so it must be a
    /// value that attribute allows: a typo must not become the first choice.
    #[test]
    fn a_composition_literal_must_fit_its_attribute() {
        let typo = COMPOSITE.replace("value=\"deferred\"", "value=\"defred\"");
        let errors = load(&format!("{PART}{typo}")).unwrap_err();
        assert!(
            errors.iter().any(|error| error
                .message
                .contains("`value=\"defred\"` is not a valid `init` for `<part>`")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_composition_must_fill_every_required_attribute() {
        let missing_cs = COMPOSITE.replace("        attr \"cs\" from=\"touch_cs\"\n", "");
        let errors = load(&format!("{PART}{missing_cs}")).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.message == "`compose \"part\"` does not fill `cs`"),
            "{errors:?}"
        );
    }
}

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
    span: Range<usize>,
}

/// The `codegen` block.
#[derive(Default)]
struct Codegen {
    feature: Option<String>,
    blocking: Option<RuntimeDecl>,
    asynchronous: Option<RuntimeDecl>,
    maps: Vec<MapDecl>,
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
                if let Some(dependency) = parse_dependency(child, &tag, errors) {
                    dependencies.push(dependency);
                }
            }
            "bus" => {
                if bus.is_some() {
                    errors.push(err(node_range(child), "`bus` given twice", "keep one"));
                } else {
                    bus = parse_bus(child, errors);
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
                 attr, compose, codegen, example or see_also",
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
    let Some(bus) = bus else {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` has no `bus`"),
            "write `bus \"i2c\" { .. }`, `bus \"spi\" { .. }` or `bus \"uart\"`",
        ));
        return;
    };
    let Some(codegen) = codegen else {
        errors.push(err(
            span.clone(),
            format!("driver `{tag}` has no `codegen`"),
            "name at least one `runtime \"blocking\"` or `runtime \"async\"` block",
        ));
        return;
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
    add_bus_attrs(&bus, &mut attrs);
    for pin in &pins {
        attrs.push(AttrDef {
            name: pin.name.clone(),
            kind: AttrKind::Ref {
                to: vec!["gpio".into()],
            },
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

    let Some(recipe) = build_recipe(
        &tag,
        &bus,
        &pins,
        &attrs,
        &codegen,
        &dependencies,
        &parts,
        errors,
    ) else {
        return;
    };

    let kind = category.clone().unwrap_or_else(|| "device".to_string());
    let recipe = DeviceRecipe {
        address: bus.address,
        address_default: bus.address_default,
        address_allowed: bus.address_allowed,
        kind,
        ..recipe
    };

    // v1 compatibility fields: `package` is the implementation crate, `expose`
    // the auxiliary ones. The emitter reads `dependencies` first; the pair is
    // kept so path resolution and lock v1 readers see the same shape.
    let package = if codegen.feature.is_none() {
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
    let default_features = prop_flag(node, "default_features", true);
    let mut features = Vec::new();

    for child in children(node) {
        match child.name().value() {
            "version" | "path" | "git" | "rev" | "default_features" => {}
            "features" => features.extend(
                positional_strings(child, errors)
                    .into_iter()
                    .map(|(feature, _)| feature),
            ),
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `dependency` `{package}`"),
                "expected version, path, git, rev, features or default_features",
            )),
        }
    }

    let chosen = [version.is_some(), path.is_some(), git.is_some()]
        .into_iter()
        .filter(|chosen| *chosen)
        .count();
    if chosen != 1 {
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
        default_features,
    })
}

fn prop_flag(node: &KdlNode, key: &str, default: bool) -> bool {
    node.entries()
        .iter()
        .filter(|entry| entry.name().is_some_and(|name| name.value() == key))
        .map(|entry| entry.value().as_bool() != Some(false))
        .next()
        .unwrap_or(default)
}

fn parse_bus(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<BusDecl> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "`bus` has no kind",
            "write `bus \"i2c\"`, `bus \"spi\"` or `bus \"uart\"`",
        ));
        return None;
    }
    let (kind, kind_span) = names.remove(0);
    if !matches!(kind.as_str(), "i2c" | "spi" | "uart") {
        errors.push(err(
            kind_span,
            format!("`{kind}` is not a bus kind"),
            "write `bus \"i2c\"`, `bus \"spi\"` or `bus \"uart\"`",
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
            other => {
                errors.push(err(
                    at,
                    format!("`{other}` is not a pin direction"),
                    "write `direction=\"output\"`, `\"input\"` or `\"bidirectional\"`",
                ));
                PinDirection::Output
            }
        },
        None => PinDirection::Output,
    };
    let level = match prop(node, "default_level", errors) {
        Some((level, at)) => {
            if !matches!(direction, PinDirection::Output) {
                errors.push(err(
                    at.clone(),
                    "`default_level` is for an output pin",
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

    Some(PinDecl {
        name,
        direction,
        level,
        pull,
        optional,
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
                "expected feature, runtime or map",
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
    let _ = asynchronous;
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
                    if !is_rust_path(&value) {
                        errors.push(err(
                            at,
                            "`endpoint` must be a Rust path",
                            "segments are identifiers separated by `::`",
                        ));
                    } else {
                        runtime.endpoint = Some(value);
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
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `runtime`"),
                "expected type, constructor, endpoint or feature",
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

    let maps: BTreeMap<&str, &MapDecl> = codegen
        .maps
        .iter()
        .map(|map| (map.name.as_str(), map))
        .collect();

    for map in &codegen.maps {
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
    let mut ctor_args = ctor_args;
    let address_seen = ctor_args.first().is_some_and(|(name, _)| name == "address");
    if matches!(bus.address, AddressNeed::Any | AddressNeed::Fixed(_)) {
        if address_seen {
            ctor_args.remove(0);
        } else {
            errors.push(err(
                ctor_span.clone(),
                format!("`{tag}` declares an address but its constructor does not take it"),
                "put `{address}` directly after the bus argument",
            ));
        }
    } else if address_seen {
        errors.push(err(
            ctor_span.clone(),
            format!("`{tag}` has no I2C address but its constructor takes `{{address}}`"),
            "add an `address` to `bus \"i2c\"` or drop the argument",
        ));
    }

    let mut args: Vec<DeviceArg> = Vec::new();
    let mut used_pins: Vec<&str> = Vec::new();
    for (name, at) in ctor_args {
        if let Some(pin) = pins.iter().find(|pin| pin.name == name) {
            used_pins.push(&pin.name);
            args.push(DeviceArg {
                name: pin.name.clone(),
                map: None,
                pin: Some(pin_template(pin.direction, pin.level, pin.pull)),
                direction: pin.direction,
                span: at,
            });
            continue;
        }
        if matches!(
            name.as_str(),
            "bus_device" | "spi_device" | "cs_pin" | "address"
        ) {
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
        args.push(DeviceArg {
            name: map.name.clone(),
            map: Some(entries),
            pin: None,
            direction: PinDirection::Output,
            span: at,
        });
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
    let (runtimes, async_mode) = match (&codegen.blocking, &codegen.asynchronous) {
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
            let async_args: Vec<&str> = async_args
                .iter()
                .map(|(name, _)| name.as_str())
                .filter(|name| *name != "address")
                .collect();
            let blocking_names: Vec<&str> = args.iter().map(|arg| arg.name.as_str()).collect();
            let async_names: Vec<&str> = async_args;
            if blocking_names != async_names {
                errors.push(err(
                    runtime.span.clone(),
                    format!("the async constructor of `{tag}` takes different arguments"),
                    "the blocking and async constructors must take the same bus, pins and slots",
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
                },
            )
        }
        (None, None) => unreachable!("checked above"),
    };

    Some(DeviceRecipe {
        bus: bus.kind.clone(),
        feature: codegen.feature.clone().unwrap_or_default(),
        kind: "device".into(),
        ty,
        new,
        deps: dependencies
            .iter()
            .map(|dependency| dependency.package.clone())
            .collect(),
        args,
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
        span: codegen.span.clone(),
    })
}

type ConstructorArgs = (String, Vec<(String, Range<usize>)>);

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
    let mut allowed: Vec<&str> = vec!["bus_device", "cs_pin", "spi_device", "address"];
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

    let mut args: Vec<(String, Range<usize>)> = Vec::new();
    for argument in &call.args {
        let Expr::Path(argument) = argument else {
            errors.push(err(
                span.clone(),
                "a constructor argument must be a `{placeholder}`",
                "write `{bus_device}`, `{address}`, a `pin` name or a mapped `attr` name",
            ));
            continue;
        };
        let Some(placeholder) = placeholder_of(&argument.path) else {
            errors.push(err(
                span.clone(),
                "a constructor argument is not a placeholder",
                "write `{bus_device}`, `{address}`, a `pin` name or a mapped `attr` name",
            ));
            continue;
        };
        let resolved = resolve_pin(ctx.pins, &placeholder)
            .map(str::to_string)
            .unwrap_or_else(|| placeholder.clone());
        if args.iter().any(|(known, _)| *known == resolved) {
            errors.push(err(
                span.clone(),
                format!("`{{{placeholder}}}` appears twice in the constructor"),
                "each constructor argument is filled once",
            ));
        }
        if matches!(placeholder.as_str(), "bus_device" | "spi_device") && !args.is_empty() {
            errors.push(err(
                span.clone(),
                "the bus argument must come first",
                "put `{bus_device}` at the front of the constructor",
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
        args.push((resolved, span.clone()));
    }
    let first = args.first().map(|(name, _)| name.as_str());
    if !matches!(first, Some("bus_device" | "spi_device")) {
        errors.push(err(
            span,
            "the constructor must take the bus first",
            "write `({bus_device}, ..)` or `({spi_device}, ..)`",
        ));
    }
    Some((new, args.into_iter().skip(1).collect()))
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
    let Ok(substituted) = substitute(text) else {
        return;
    };
    if let Err(error) = syn::parse_str::<Type>(&substituted) {
        errors.push(err(span, "`type` is not a Rust type", error.to_string()));
    }
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
                .contains("`default_level` is for an output pin")),
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
                .contains("`default_level` is for an output pin")),
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

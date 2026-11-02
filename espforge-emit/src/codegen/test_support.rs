//! Fixtures shared by the codegen backends' unit tests.
//!
//! `wifi` and `embassy` are derived flags: they follow `# peripherals` and the
//! runtime, and setting them by hand is a resolver error. So the fixture takes
//! the runtime and the peripheral list, not an option flag for them.

use espforge_elements::{
    Answers, Board, Console, DerivedInputs, Device, Ident, Meta, ResolvedOptions, ResourceGraph,
    Runtime, Service, resolve,
};
use espforge_schema::schema;

use super::chip::Chip;

pub struct Case {
    pub meta: Meta,
    pub opts: ResolvedOptions,
}

/// A resolved project: `hello world` on `chip`, with `flags` applied.
///
/// `defaults` is the `<options defaults=...>` switch; `peripherals` are the
/// tags in `# peripherals` (a `<wifi>` there is the only way to turn `wifi`
/// on).
pub fn case(
    chip: &str,
    runtime: Runtime,
    defaults: bool,
    flags: &[(&str, &str)],
    peripherals: &[&str],
) -> Case {
    let meta = Meta {
        runtime,
        ..meta(chip)
    };
    let mut answers = Answers::empty(0..0);
    answers.defaults = defaults;
    for (name, value) in flags {
        answers
            .apply_flag(schema(), name, value)
            .unwrap_or_else(|e| panic!("option `{name}={value}`: {e:?}"));
    }
    let derived = DerivedInputs {
        peripherals: peripherals
            .iter()
            .map(|name| (name.to_string(), 0..1))
            .collect(),
        console_span: None,
    };
    let opts = resolve(&meta, &answers, &derived)
        .unwrap_or_else(|e| panic!("resolving the fixture: {e:?}"));
    Case { meta, opts }
}

pub fn chip(name: &str) -> Chip {
    Chip::resolve(name).unwrap()
}

/// A board with no hardware: enough for a backend that only reads `meta`.
pub fn board() -> Board {
    Board {
        meta: meta("esp32c3"),
        resources: ResourceGraph::default(),
        components: Vec::new(),
        devices: Vec::new(),
        services: Vec::new(),
        warnings: Vec::new(),
    }
}

/// One lowered device, as a `device_recipe` builds it.
///
/// Only the two fields a manifest reads are interesting here: the feature the
/// project asks `espforge-drivers` for, and the crates the driver needs named.
pub fn device(feature: &'static str, deps: &[&str]) -> Device {
    Device {
        id: Ident::new(feature, 0..0),
        tag: feature,
        bus_component: Ident::new("bus", 0..0),
        feature: feature.to_string(),
        deps: deps.iter().map(|dep| (*dep).to_string()).collect(),
        ty: format!("espforge_drivers::{feature}"),
        new: format!("espforge_drivers::{feature}::new"),
        takes_bus: false,
        taken: None,
        pins: Vec::new(),
        args: Vec::new(),
    }
}

/// One lowered service, as a `service_recipe` builds it.
///
/// Only the fields a backend reads are interesting here: the runtime feature
/// the manifest asks for, and the heap the service needs.
pub fn service(tag: &'static str, feature: &str, heap_bytes: Option<u32>) -> Service {
    Service {
        id: Ident::new(tag, 0..0),
        tag,
        network: Ident::new("home", 0..0),
        feature: feature.to_string(),
        ty: format!("espforge_runtime::{tag}"),
        new: format!("espforge_runtime::{tag}::new"),
        config: None,
        heap_bytes,
        config_fields: Vec::new(),
        extra_args: Vec::new(),
    }
}

/// The project settings every fixture starts from.
pub fn meta(chip: &str) -> Meta {
    Meta {
        name: "hello world".into(),
        description: Some("demo".into()),
        chip: chip.into(),
        runtime: Runtime::Blocking,
        console: Console::Uart,
    }
}

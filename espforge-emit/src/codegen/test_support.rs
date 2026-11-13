//! Fixtures shared by the codegen backends' unit tests.
//!
//! `wifi` and `embassy` are derived flags: they follow `# peripherals` and the
//! runtime, and setting them by hand is a resolver error. So the fixture takes
//! the runtime and the peripheral list, not an option flag for them.

use espforge_elements::{
    Answers, Board, BusDevice, BusId, BusTransport, Console, DerivedInputs, Device, Driver,
    I2cTransport, Ident, Meta, ProtocolService, ResolvedOptions, ResourceGraph, Runtime, Service,
    resolve,
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
        devices: Vec::new(),
        services: Vec::new(),
        warnings: Vec::new(),
    }
}

/// One lowered device, as a `device_recipe` builds it.
///
/// Only the two fields a manifest reads are interesting here: the feature the
/// project asks `espforge-drivers` for, and the crates the driver needs named.
/// The device sits on an I2C bus at slot 0; a test that emits its field must
/// put that bus on the board.
pub fn device(feature: &'static str, deps: &[&str]) -> Device {
    Device::Bus(BusDevice {
        id: Ident::new(feature, 0..0),
        tag: feature.into(),
        driver: Driver {
            feature: feature.to_string(),
            deps: deps.iter().map(|dep| (*dep).to_string()).collect(),
            ty: espforge_elements::model::RustType::parse(&format!("espforge_drivers::{feature}"))
                .expect("a fixture device type"),
            new: espforge_elements::model::RustPath::parse(&format!(
                "espforge_drivers::{feature}::new"
            ))
            .expect("a fixture constructor"),
            pins: Vec::new(),
            args: Vec::new(),
            factory_args: Vec::new(),
            runtimes: espforge_elements::RuntimeSet::BLOCKING,
            init: espforge_elements::model::Init::Boot,
            async_ty: None,
            async_new: None,
            async_feature: None,
            package: None,
            expose: Vec::new(),
            dependencies: Vec::new(),
            manifest: None,
        },
        transport: BusTransport::I2c(I2cTransport {
            bus: BusId(0),
            address: 0x3C,
            endpoint: None,
            async_endpoint: None,
        }),
    })
}

/// One lowered service, as a `service_recipe` builds it.
///
/// Only the fields a backend reads are interesting here: the runtime feature
/// the manifest asks for, and the heap the service needs.
pub fn service(tag: &'static str, feature: &str, heap_bytes: Option<u32>) -> Service {
    // A fixture tag mirrors a manifest tag, which may carry a hyphen; a Rust
    // type path may not.
    let name = tag.replace('-', "_");
    Service::Protocol(ProtocolService {
        id: Ident::new(tag, 0..0),
        tag: tag.into(),
        network: Ident::new("home", 0..0),
        feature: feature.to_string(),
        ty: espforge_elements::model::RustType::parse(&format!("espforge_runtime::{name}"))
            .expect("a fixture service type"),
        new: espforge_elements::model::RustPath::parse(&format!("espforge_runtime::{name}::new"))
            .expect("a fixture constructor"),
        storage: None,
        config: None,
        heap_bytes,
        config_fields: Vec::new(),
        extra_args: Vec::new(),
    })
}

/// The project settings every fixture starts from.
pub fn meta(chip: &str) -> Meta {
    Meta {
        name: "hello world".into(),
        description: Some("demo".into()),
        chip: chip.into(),
        runtime: Runtime::Blocking,
        console: Console::Uart,
        ota: None,
        logger: None,
    }
}

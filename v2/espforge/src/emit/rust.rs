//! Rust source emitters (ADR-005): pure `fn(&DeviceTree) -> Vec<Artifact>`.
//!
//! These produce the espforge-owned layers of the generated project:
//! - `Cargo.toml`         — depends on `espforge-runtime`; uses a path dep when
//!                           `ESPFORGE_USE_LOCAL` is set (v1 behaviour).
//! - `src/generated.rs`   — `PeripheralRegistry`, `Components`, `Devices` with
//!                           one field per instance, built move-by-value (ADR-008).
//! - `src/lib.rs`         — `#![no_std]` re-exports + `component!`/`device!` macros.
//! - `src/bin/main.rs`    — entry point (blocking or embassy from IR flags).
//! - `src/app.rs`         — user-owned skeleton (`setup`/`forever` hooks).
//!
//! The emitters reference `espforge-runtime` only by *name* in the emitted
//! token stream (ADR-007) — never as a Cargo dependency of the `espforge` host
//! crate.

use anyhow::Result;
use espforge_model::ir::{DeviceTree, Runtime, Tier};
use espforge_model::value::Artifact;
use std::env;

/// Environment variable (v1 behaviour): when set to a local espforge checkout,
/// generated projects depend on local path copies of `espforge-runtime` instead
/// of the published crates.io version.
pub const ESPFORGE_USE_LOCAL: &str = "ESPFORGE_USE_LOCAL";

/// Resolve an `espforge-*` dependency declaration for the generated project's
/// `Cargo.toml`. By default it uses the published crates.io version; when
/// `ESPFORGE_USE_LOCAL` is set (v1 behaviour) it flips to a local path dep into
/// that checkout. Applies uniformly to `espforge-runtime`, `espforge-bindings`,
/// `espforge-model`, etc.
fn espforge_dep(crate_name: &str) -> String {
    match env::var(ESPFORGE_USE_LOCAL) {
        Ok(path) if !path.trim().is_empty() => {
            format!(
                "{crate_name} = {{ path = \"{}/{crate_name}\" }}",
                path.trim_end_matches('/')
            )
        }
        _ => format!("{crate_name} = \"0.1\""),
    }
}

pub fn emit(ir: &DeviceTree) -> Result<Vec<Artifact>> {
    let catalog = espforge_bindings::registry();
    let ctx = espforge_model::driver::GenContext {
        target: ir.meta.target.clone(),
        is_embassy: ir.flags.is_embassy,
        peripherals: ir.peripherals.clone(),
    };

    // Driver-driven construction: ask each instance's driver for its wiring
    // snippet (ADR-006/008). Emitted only via the catalog, so adding a driver
    // is a one-file change.
    let mut component_inits = Vec::new();
    let mut device_inits = Vec::new();
    for inst in &ir.instances {
        match catalog.get(&inst.kind) {
            Some(driver) => {
                let c = driver.construct(inst, &ctx);
                let line = format!("        {}: {},", c.field, c.expr);
                match inst.tier {
                    Tier::Component => component_inits.push(line),
                    Tier::Device => device_inits.push(line),
                }
            }
            None => {
                anyhow::bail!("no driver registered for kind `{}`", inst.kind);
            }
        }
    }

    let mut out = Vec::new();
    out.push(Artifact::owned("Cargo.toml", emit_cargo_toml(ir)?));
    out.push(Artifact::owned("src/generated.rs", emit_generated(ir, &catalog)));
    out.push(Artifact::owned("src/lib.rs", emit_lib(ir)));
    out.push(Artifact::owned(
        "src/bin/main.rs",
        emit_main(ir, &component_inits.join("\n"), &device_inits.join("\n")),
    ));
    out.push(Artifact::seed_once("src/app.rs", emit_app(ir)));
    Ok(out)
}

fn project_name(ir: &DeviceTree) -> String {
    ir.meta
        .name
        .clone()
        .unwrap_or_else(|| "espforge_project".into())
        .replace('-', "_")
}

fn emit_cargo_toml(ir: &DeviceTree) -> Result<String> {
    let name = project_name(ir);
    let embassy = if ir.flags.is_embassy { "embassy-executor = \"*\"\n" } else { "" };
    let alloc = if ir.flags.has_alloc { "embedded-alloc = \"*\"\n" } else { "" };
    let runtime_dep = espforge_dep("espforge-runtime");
    Ok(format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
{runtime_dep}
esp-hal = "*"
esp-backtrace = "*"
esp-println = "*"
static_cell = "*"
{embassy}{alloc}

[profile.release]
debug = true
"#
    ))
}

fn emit_generated(ir: &DeviceTree, catalog: &espforge_model::driver::Registry) -> String {
    let component_fields = ir
        .instances
        .iter()
        .filter(|i| i.tier == Tier::Component)
        .map(|i| {
            let id = sanitize(&i.id);
            let ty = catalog
                .get(&i.kind)
                .map(|d| d.type_name().to_string())
                .unwrap_or_else(|| i.kind.clone());
            format!("    pub {id}: espforge_runtime::components::{ty},")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let device_fields = ir
        .instances
        .iter()
        .filter(|i| i.tier == Tier::Device)
        .map(|i| {
            let id = sanitize(&i.id);
            let ty = catalog
                .get(&i.kind)
                .map(|d| d.type_name().to_string())
                .unwrap_or_else(|| i.kind.clone());
            format!("    pub {id}: espforge_runtime::devices::{ty},")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project YAML.
#![allow(dead_code)]

/// Raw ESP32 peripherals, moved out of `esp_hal::Peripherals` once at init.
pub struct PeripheralRegistry {{
    pub peripherals: esp_hal::Peripherals,
}}

impl PeripheralRegistry {{
    pub fn new(peripherals: esp_hal::Peripherals) -> Self {{
        Self {{ peripherals }}
    }}
}}

/// Constructed components. Each field is a concrete `espforge_runtime`
/// capability, wired move-by-value from the registry (ADR-008).
pub struct Components {{
{component_fields}
}}

/// Constructed terminal devices.
pub struct Devices {{
{device_fields}
}}
"#
    )
}

fn emit_lib(_ir: &DeviceTree) -> String {
    r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project YAML.
#![no_std]

pub mod generated;
pub mod app;

pub use generated::{Components, Devices, PeripheralRegistry};

/// Access a component from the generated `Components` struct.
#[macro_export]
macro_rules! component {
    ($name:ident) => {
        &$crate::CTX.components.$name
    };
}

/// Access a device from the generated `Devices` struct.
#[macro_export]
macro_rules! device {
    ($name:ident) => {
        &$crate::CTX.devices.$name
    };
}

/// The whole runtime context the app receives (ADR-008: stable app-facing API).
pub struct Context {
    pub logger: espforge_runtime::Logger,
    pub delay: espforge_runtime::Delay,
    pub components: Components,
    pub devices: Devices,
}

/// Set by the entry point once wiring is complete.
pub static mut CTX: Option<Context> = None;
"#
    .to_string()
}

fn emit_main(ir: &DeviceTree, component_inits: &str, device_inits: &str) -> String {
    match ir.meta.runtime {
        Runtime::Embassy => emit_main_embassy(ir, component_inits, device_inits),
        Runtime::Blocking => emit_main_blocking(ir, component_inits, device_inits),
    }
}

fn emit_main_blocking(_ir: &DeviceTree, component_inits: &str, device_inits: &str) -> String {
    format!(
        r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project YAML.
#![no_std]
#![no_main]

use esp_backtrace as _;
use static_cell::StaticCell;

#[esp_hal::main]
fn main() -> ! {{
    esp_println::logger::init_logger_from_env();

    let peripherals = esp_hal::init(esp_hal::Config::default());
    let registry = PeripheralRegistry::new(peripherals);

    // Components are wired move-by-value from the registry (ADR-008).
    let components = Components {{
{component_inits}
    }};
    let devices = Devices {{
{device_inits}
    }};

    let logger = espforge_runtime::Logger::new();
    let delay = espforge_runtime::Delay::new();
    let ctx = Context {{ logger, delay, components, devices }};
    unsafe {{ crate::CTX = Some(ctx); }}

    let ctx = unsafe {{ crate::CTX.as_mut().unwrap() }};
    app::setup(ctx);
    loop {{
        app::forever(ctx);
    }}
}}
"#
    )
}

fn emit_main_embassy(_ir: &DeviceTree, component_inits: &str, device_inits: &str) -> String {
    format!(
        r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project YAML.
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::timer::timg::TimerGroup;
use static_cell::StaticCell;

#[esp_rtos::main]
async fn main(spawner: Spawner) {{
    esp_println::logger::init_logger_from_env();

    let peripherals = esp_hal::init(esp_hal::Config::default());
    let registry = PeripheralRegistry::new(peripherals);

    let sw_int = SoftwareInterruptControl::new(registry.peripherals.sw_interrupt);
    let timg0 = TimerGroup::new(registry.peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let components = Components {{
{component_inits}
    }};
    let devices = Devices {{
{device_inits}
    }};

    let logger = espforge_runtime::Logger::new();
    let delay = espforge_runtime::Delay::new();
    let ctx = Context {{ logger, delay, components, devices }};
    unsafe {{ crate::CTX = Some(ctx); }}

    let ctx = unsafe {{ crate::CTX.as_mut().unwrap() }};
    app::setup(ctx, spawner).await;
    loop {{
        app::forever(ctx).await;
    }}
}}
"#
    )
}

fn emit_app(ir: &DeviceTree) -> String {
    let (setup_sig, forever_sig) = if ir.flags.is_embassy {
        (
            "pub async fn setup(ctx: &mut crate::Context, _spawner: Spawner)",
            "pub async fn forever(_ctx: &mut crate::Context)",
        )
    } else {
        (
            "pub fn setup(_ctx: &mut crate::Context)",
            "pub fn forever(_ctx: &mut crate::Context)",
        )
    };
    let embassy_use = if ir.flags.is_embassy {
        "use embassy_executor::Spawner;\n"
    } else {
        ""
    };
    format!(
        r#"// USER-OWNED. This file is NOT regenerated by espforge. Edit freely.
{embassy_use}
{setup_sig} {{
    // espforge wired your components/devices; access them via `component!`/`device!`.
}}

{forever_sig} {{
    // your loop body here
}}
"#
    )
}

/// Sanitize an instance id into a Rust identifier.
fn sanitize(id: &str) -> String {
    let mut out = String::new();
    for (i, c) in id.chars().enumerate() {
        if c.is_alphanumeric() && (i == 0 && c.is_alphabetic() || i > 0) {
            out.push(c);
        } else if i > 0 {
            out.push('_');
        }
    }
    if out.is_empty() {
        out = "inst".to_string();
    }
    out
}

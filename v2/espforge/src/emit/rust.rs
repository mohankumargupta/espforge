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
use std::path::Path;

/// Environment variable (v1 behaviour): when set to a truthy value (`"true"`,
/// `ESPFORGE_USE_LOCAL=true`), generated projects depend on local path copies
/// of `espforge-runtime` instead of the published crates.io version. Fed by
/// the `justfile` that `setup`/`create` emit (ADR-001/§17.1).
pub const ESPFORGE_USE_LOCAL: &str = "ESPFORGE_USE_LOCAL";

/// When `ESPFORGE_USE_LOCAL` is set, this names the `espforge` binary whose
/// containing `v2` checkout provides the local path deps. If unset, we fall
/// back to the location of the running `espforge` executable.
pub const ESPFORGE_BINARY: &str = "ESPFORGE_BINARY";

/// When `ESPFORGE_USE_LOCAL` is set, this names the espforge checkout root
/// (the `v2` directory) to use for local path deps, already expressed relative
/// to the generated project's `out` directory. Set by the `build` subcommand
/// from the `path:` in `answers.yaml` (which is relative to the project dir),
/// re-based to `out`. Takes precedence over `ESPFORGE_BINARY`/`current_exe`.
pub const ESPFORGE_PATH: &str = "ESPFORGE_PATH";

/// Resolve an `espforge-*` dependency declaration for the generated project's
/// `Cargo.toml`. By default it uses the published crates.io version; when
/// `ESPFORGE_USE_LOCAL` is truthy it flips to a local path dep rooted at the
/// `v2` directory of the espforge checkout (derived from `ESPFORGE_BINARY`, or
/// the running executable). Applies uniformly to `espforge-runtime`,
/// `espforge-bindings`, `espforge-model`, etc.
fn espforge_dep(crate_name: &str) -> String {
    const VERSION: &str = "0.1.0";
    if !use_local() {
        return format!("{crate_name} = \"{VERSION}\"");
    }
    let root = v2_root();
    format!(
        "{crate_name} = {{ path = \"{root}/{crate_name}\", version = \"{VERSION}\" }}"
    )
}

/// `true` iff `ESPFORGE_USE_LOCAL` is set to a truthy value ("true"/"1"/"yes").
fn use_local() -> bool {
    match env::var(ESPFORGE_USE_LOCAL) {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            matches!(t.as_str(), "true" | "1" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// Derive the `v2` root of the espforge checkout. `ESPFORGE_BINARY` points at
/// the espforge binary (e.g. `/repo/v2/target/debug/espforge`); we walk up to
/// the nearest ancestor named `v2`, falling back to the binary's parent and then
/// the running executable's parent.
/// Derive the `v2` root of the espforge checkout, preferring an explicit
/// `ESPFORGE_PATH` (the checkout, relative to `out`), then the `v2` ancestor of
/// `ESPFORGE_BINARY`, then the running executable's `v2` ancestor.
fn v2_root() -> String {
    if let Ok(path) = env::var(ESPFORGE_PATH) {
        if !path.is_empty() {
            return normalize(Path::new(&path));
        }
    }
    let candidates: Vec<std::path::PathBuf> = [
        env::var(ESPFORGE_BINARY).ok(),
        env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned()),
    ]
    .into_iter()
    .flatten()
    .map(|p| Path::new(&p).to_path_buf())
    .collect();

    for bin in &candidates {
        if let Some(root) = walk_up_to_v2(bin) {
            return normalize(&root);
        }
    }
    // Last resort: parent of the running executable.
    if let Some(exe) = env::current_exe().ok() {
        if let Some(parent) = exe.parent() {
            return normalize(parent);
        }
    }
    ".".to_string()
}

/// Walk `start` upward until an ancestor directory is named `v2`; return it.
fn walk_up_to_v2(start: &Path) -> Option<std::path::PathBuf> {
    // If the path is itself a file, start from its parent.
    let mut cur = if start.is_dir() { start.to_path_buf() } else { start.parent()?.to_path_buf() };
    loop {
        if cur.file_name().map(|n| n == "v2").unwrap_or(false) {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn normalize(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

pub fn emit(ir: &DeviceTree) -> Result<Vec<Artifact>> {
    let catalog = espforge_bindings::registry();
    let ctx = espforge_model::driver::GenContext {
        target: ir.meta.target.clone(),
        is_embassy: ir.flags.is_embassy,
        peripherals: ir.peripherals.clone(),
        backend: &espforge_model::backend::BLOCKING,
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
    Ok(out)
}

fn project_name(ir: &DeviceTree) -> String {
    ir.meta
        .name
        .clone()
        .unwrap_or_else(|| "espforge_project".into())
        .replace('-', "_")
}

//produces Cargo.toml
fn emit_cargo_toml(ir: &DeviceTree) -> Result<String> {
    let name = project_name(ir);
    let chip = ir
        .meta
        .target
        .clone()
        .unwrap_or_else(|| "esp32c3".to_string());
    let embassy = if ir.flags.is_embassy { "embassy-executor = \"*\"\n" } else { "" };
    let alloc = if ir.flags.has_alloc { "embedded-alloc = \"*\"\n" } else { "" };
    let runtime_dep = espforge_dep("espforge-runtime");
    // esp-hal + friends need the chip feature enabled (e.g. "esp32c3"); esp-hal
    // also needs "embassy" when using the async runtime (ADR-008 flags).
    let mut hal_feats = vec![chip.clone()];
    if ir.flags.is_embassy {
        hal_feats.push("embassy".to_string());
    }
    let hal_feats = hal_feats.join("\", \"");
    Ok(format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
{runtime_dep}
esp-hal = {{ version = "*", features = ["{hal_feats}"] }}
esp-backtrace = {{ version = "*", features = ["{chip}", "println", "panic-handler"] }}
esp-println = {{ version = "*", features = ["{chip}", "log-04"] }}
static_cell = "*"
{embassy}{alloc}

# Name the binary explicitly so it matches the package name (and the path
# wokwi.toml points at). Without this, Cargo names the binary after the
# `src/bin/main.rs` file stem ("main"), producing no `blink` binary.
[[bin]]
name = "{name}"
path = "src/bin/main.rs"

[profile.release]
debug = true

# Standalone workspace root: keeps this generated project self-contained and
# prevents cargo from walking up to a parent workspace (ADR-008 generated output).
[workspace]
"#
    ))
}

//produces src/generated.rs
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

/// Raw ESP32 peripherals, moved out of `esp_hal::peripherals::Peripherals` once at init.
pub struct PeripheralRegistry {{
    pub peripherals: esp_hal::peripherals::Peripherals,
}}

impl PeripheralRegistry {{
    pub fn new(peripherals: esp_hal::peripherals::Peripherals) -> Self {{
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

/// Access a component from a `Context`'s generated `Components` struct.
/// Usage: `component!(ctx, my_led)`
#[macro_export]
macro_rules! component {
    ($ctx:expr, $name:ident) => {
        &mut $ctx.components.$name
    };
}

/// Access a device from a `Context`'s generated `Devices` struct.
/// Usage: `device!(ctx, my_screen)`
#[macro_export]
macro_rules! device {
    ($ctx:expr, $name:ident) => {
        &mut $ctx.devices.$name
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
pub static CTX: static_cell::StaticCell<Context> = static_cell::StaticCell::new();
"#
    .to_string()
}

fn emit_main(ir: &DeviceTree, component_inits: &str, device_inits: &str) -> String {
    let name = project_name(ir);
    match ir.meta.runtime {
        Runtime::Embassy => emit_main_embassy(ir, &name, component_inits, device_inits),
        Runtime::Blocking => emit_main_blocking(ir, &name, component_inits, device_inits),
    }
}

fn emit_main_blocking(_ir: &DeviceTree, name: &str, component_inits: &str, device_inits: &str) -> String {
    format!(
        r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project YAML.
#![no_std]
#![no_main]

use esp_backtrace as _;
use {name}::{{app, Components, Context, Devices, PeripheralRegistry, CTX}};

#[esp_hal::main]
fn main() -> ! {{
    esp_println::logger::init_logger_from_env();
    esp_println::print!("\x1b[20h");
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
    let ctx = CTX.init(ctx);
    app::setup(ctx);
    loop {{
        app::forever(ctx);
    }}
}}
"#
    )
}

fn emit_main_embassy(_ir: &DeviceTree, name: &str, component_inits: &str, device_inits: &str) -> String {
    format!(
        r#"// GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project YAML.
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::timer::timg::TimerGroup;
use {name}::{{app, Components, Context, Devices, PeripheralRegistry, CTX}};

#[esp_rtos::main]
async fn main(spawner: Spawner) {{
    esp_println::logger::init_logger_from_env();
    esp_println::print!("\x1b[20h");
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
    let ctx = CTX.init(ctx);
    app::setup(ctx, spawner).await;
    loop {{
        app::forever(ctx).await;
    }}
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

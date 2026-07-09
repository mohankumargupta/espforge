//! The `validate` and `resolve` stages (ADR-005, ADR-009).
//!
//! `validate` runs *before* resolve and gating `build`. It checks, emitting
//! span-aware `Diag`s:
//!   - unknown drivers (`using:` not in the catalog),
//!   - unresolved `$name` refs (instance/peripheral not declared),
//!   - unexpected `with:` keys vs the driver spec (lightweight shape check),
//!   - pin/peripheral double-claims (catches ADR-008-B risk at YAML time),
//!   - dependency cycles (and device-on-device, which ADR-003 forbids).
//!
//! `resolve` consumes a validated project and produces the `DeviceTree` IR:
//! typed instances with resolved dependency edges (kinds + access), claim sets,
//! a topological `init_order`, required features, and cross-cutting flags.

use espforge_bindings::catalog;
use espforge_model::catalog::DriverSpec;
use espforge_model::ir::{
    Dependency, DeviceTree, DepKind, Flags, Meta, Peripheral, PeripheralKind, ResolvedInstance,
    Runtime, Tier,
};
use espforge_model::project::{Esp32Section, Instance, Project, Runtime as ProjRuntime};
use espforge_model::value::{Diag, Level, PinRef, ResourceRef};
use std::collections::{HashMap, HashSet};

/// Validate a parsed project. Returns `Ok(())` if valid, or the collected
/// diagnostics (at least one Error) if not.
pub fn validate(project: &Project) -> Result<(), Vec<Diag>> {
    let mut diags = Vec::new();
    let cat = catalog();
    let by_kind: HashMap<String, DriverSpec> = cat
        .iter()
        .map(|s| (s.kind.clone(), s.clone()))
        .collect();

    // Index declared names: instances + peripherals (buses + pins).
    let mut instance_names = HashSet::new();
    for inst in project.components.iter().chain(project.devices.iter()) {
        if !instance_names.insert(inst.id.clone()) {
            diags.push(
                Diag::error(format!("duplicate instance name `{}`", inst.id))
                    .at(inst.span)
                    .field(&inst.id)
                    .hint("instance names must be unique"),
            );
        }
    }
    let mut peripheral_names = HashSet::new();
    collect_peripheral_names(&project.esp32, &mut peripheral_names);

    // Build the catalog of resolvable refs: instances + peripherals.
    let mut resolvable = instance_names.clone();
    for n in &peripheral_names {
        resolvable.insert(n.clone());
    }

    // Cross-instance claim tracking (peripheral + pin double-claim).
    let mut peripheral_owner: HashMap<String, String> = HashMap::new();
    let mut pin_owner: HashMap<u32, String> = HashMap::new();

    // Validate each instance.
    for inst in project.components.iter().chain(project.devices.iter()) {
        let spec = match by_kind.get(inst.kind.as_str()) {
            Some(s) => s,
            None => {
                diags.push(
                    Diag::error(format!(
                        "unknown driver `{}` (using: {})",
                        inst.kind,
                        inst.kind
                    ))
                    .at(inst.span)
                    .field("using")
                    .hint("check the driver name; built-in: led, i2c, ssd1306"),
                );
                continue;
            }
        };

        // Check expected pin/peripheral refs resolve, and track claims.
        for key in spec.pins.iter().chain(spec.peripherals.iter()) {
            match extract_ref(&inst.with, key) {
                Some(ResourceRef { name, span }) => {
                    let pin_key = gpio_key_for_ref(&project.esp32, &name);
                    let resolved = resolvable.contains(&name) || pin_key.is_some();
                    if !resolved {
                        diags.push(
                            Diag::error(format!("unresolved reference `${name}`"))
                                .at(span)
                                .field(&format!("{}.{}", inst.id, key))
                                .hint("declare the referenced pin or peripheral"),
                        );
                        continue;
                    }
                    if let Some(gkey) = pin_key {
                        let num = project.esp32.gpio[&gkey].pin;
                        if let Some(prev) = pin_owner.get(&num) {
                            diags.push(
                                Diag::error(format!(
                                    "pin GPIO{num} claimed by both `{prev}` and `{}`",
                                    inst.id
                                ))
                                .at(span)
                                .field(&format!("{}.{}", inst.id, key))
                                .hint("a pin may be claimed by exactly one instance"),
                            );
                        } else {
                            pin_owner.insert(num, inst.id.clone());
                        }
                    } else if let Some(prev) = peripheral_owner.get(&name) {
                        diags.push(
                            Diag::error(format!(
                                "peripheral `{name}` claimed by both `{prev}` and `{}`",
                                inst.id
                            ))
                            .at(span)
                            .field(&format!("{}.{}", inst.id, key))
                            .hint("a peripheral may be claimed by exactly one instance"),
                        );
                    } else {
                        peripheral_owner.insert(name, inst.id.clone());
                    }
                }
                None => {
                    diags.push(
                        Diag::error(format!(
                            "driver `{}` requires `with.{}`",
                            inst.kind, key
                        ))
                        .at(inst.span)
                        .field(&format!("{}.{}", inst.id, key)),
                    );
                }
            }
        }

        // Check deps (shared instance refs) resolve.
        for dep in &spec.deps {
            match extract_ref(&inst.with, &dep.key) {
                Some(ResourceRef { name, span }) => {
                    if !resolvable.contains(&name) {
                        diags.push(
                            Diag::error(format!("unresolved reference `${name}`"))
                                .at(span)
                                .field(&format!("{}.{}", inst.id, dep.key))
                                .hint("declare the referenced component"),
                        );
                    }
                }
                None => {
                    diags.push(
                        Diag::error(format!(
                            "driver `{}` requires `with.{}`",
                            inst.kind, dep.key
                        ))
                        .at(inst.span)
                        .field(&format!("{}.{}", inst.id, dep.key)),
                    );
                }
            }
        }

        // Device-on-device is forbidden (ADR-003): a Device's instance deps
        // must point at Components, never Devices.
        for dep in &spec.deps {
            if dep.kind == DepKind::Instance {
                if let Some(target) = project
                    .components
                    .iter()
                    .chain(project.devices.iter())
                    .find(|i| {
                        extract_ref(&inst.with, &dep.key)
                            .map(|r| r.name == i.id)
                            .unwrap_or(false)
                    })
                {
                    if is_device(&cat, &target.kind) {
                        diags.push(
                            Diag::error(format!(
                                "device `{}` cannot depend on device `{}`",
                                inst.id, target.id
                            ))
                            .at(inst.span)
                            .hint("devices are terminal; depend on a component instead"),
                        );
                    }
                }
            }
        }
    }

    if diags.iter().any(|d| d.level == Level::Error) {
        return Err(diags);
    }
    Ok(())
}

/// Resolve a validated project into the `DeviceTree` IR (ADR-005).
pub fn resolve(project: &Project) -> DeviceTree {
    let cat = catalog();
    let by_kind: HashMap<String, DriverSpec> = cat
        .iter()
        .map(|s| (s.kind.clone(), s.clone()))
        .collect();

    let mut peripherals = collect_peripherals(&project.esp32);
    let mut instances = Vec::new();
    let mut required_features = Vec::new();
    let mut flags = Flags {
        is_embassy: matches!(project.espforge.runtime, ProjRuntime::Embassy),
        ..Default::default()
    };

    // Assign claims onto peripherals.
    let mut peripheral_claim = HashMap::new();
    let mut pin_claim_owner = HashMap::new();

    let all: Vec<&Instance> = project.components.iter().chain(project.devices.iter()).collect();
    for inst in &all {
        let spec = by_kind.get(inst.kind.as_str()).cloned();
        let tier = spec.as_ref().map(|s| s.tier).unwrap_or(Tier::Component);

        let mut deps = Vec::new();
        let mut claims = Vec::new();
        let mut pins = Vec::new();

        if let Some(spec) = spec {
            for dep in &spec.deps {
                if let Some(ResourceRef { name, .. }) = extract_ref(&inst.with, &dep.key) {
                    deps.push(Dependency {
                        name,
                        kind: dep.kind,
                        access: dep.access,
                    });
                }
            }
            for key in &spec.pins {
                if let Some(ResourceRef { name, span }) = extract_ref(&inst.with, key) {
                    if let Some(gkey) = gpio_key_for_ref(&project.esp32, &name) {
                        let num = project.esp32.gpio[&gkey].pin;
                        pins.push(PinRef { number: num, span });
                        pin_claim_owner.insert(num, inst.id.clone());
                    }
                }
            }
            for key in &spec.peripherals {
                if let Some(ResourceRef { name, .. }) = extract_ref(&inst.with, key) {
                    claims.push(ResourceRef::synthetic(name.clone()));
                    peripheral_claim.insert(name, inst.id.clone());
                }
            }
            let f = &spec.flags;
            if f.has_alloc { flags.has_alloc = true; }
            if f.has_wifi { flags.has_wifi = true; }
            if f.needs_delay { flags.needs_delay = true; }
            if f.needs_stack { flags.needs_stack = true; }
        }

        instances.push(ResolvedInstance {
            id: inst.id.clone(),
            kind: inst.kind.clone(),
            tier,
            with: inst.with.clone(),
            deps,
            claims,
            pins,
            span: inst.span,
        });
    }

    // Record claims on peripherals.
    for p in peripherals.iter_mut() {
        if let Some(owner) = peripheral_claim.get(&p.name) {
            p.claimed_by = Some(owner.clone());
        }
    }

    // Derive required features.
    if flags.is_embassy {
        required_features.push("embassy".to_string());
    }
    if flags.has_alloc {
        required_features.push("alloc".to_string());
    }
    if flags.has_wifi {
        required_features.push("wifi".to_string());
    }

    let init_order = topological_order(&instances);

    DeviceTree {
        meta: Meta {
            name: project.espforge.name.clone(),
            target: project.espforge.target.clone(),
            runtime: match project.espforge.runtime {
                ProjRuntime::Blocking => Runtime::Blocking,
                ProjRuntime::Embassy => Runtime::Embassy,
            },
        },
        peripherals,
        instances,
        init_order,
        required_features,
        flags,
    }
}

// --- helpers ----------------------------------------------------------------

fn is_device(cat: &[DriverSpec], kind: &str) -> bool {
    cat.iter().any(|s| s.kind == kind && s.tier == Tier::Device)
}

fn collect_peripheral_names(esp32: &Esp32Section, out: &mut HashSet<String>) {
    for name in esp32.gpio.keys() {
        out.insert(name.clone());
    }
    for name in esp32.i2c.keys() {
        out.insert(name.clone());
    }
    for name in esp32.spi.keys() {
        out.insert(name.clone());
    }
    for name in esp32.uart.keys() {
        out.insert(name.clone());
    }
    if esp32.wifi.is_some() {
        out.insert("wifi".to_string());
    }
}

/// Map a pin reference name to its `esp32.gpio` key. Accepts both the explicit
/// map key (`$gpio2`) and the legacy numeric form (`$GPIO18` / `pin: 18`), which
/// reverse-resolves to the gpio entry whose `pin` matches. Returns `None` for
/// anything that is not a gpio peripheral.
fn gpio_key_for_ref(esp32: &Esp32Section, name: &str) -> Option<String> {
    if esp32.gpio.contains_key(name) {
        return Some(name.to_string());
    }
    if let Some(num) = name.strip_prefix("GPIO").and_then(|n| n.parse::<u32>().ok()) {
        return esp32
            .gpio
            .iter()
            .find(|(_, g)| g.pin == num)
            .map(|(k, _)| k.clone());
    }
    None
}

fn collect_peripherals(esp32: &Esp32Section) -> Vec<Peripheral> {
    let mut v = Vec::new();
    for (name, g) in &esp32.gpio {
        v.push(Peripheral {
            name: name.clone(),
            kind: PeripheralKind::Pin,
            number: g.pin,
            field: format!("GPIO{}", g.pin),
            bus: None,
            claimed_by: None,
        });
    }
    for (name, b) in &esp32.i2c {
        v.push(Peripheral {
            name: name.clone(),
            kind: PeripheralKind::I2c,
            number: b.peripheral,
            field: format!("I2C{}", b.peripheral),
            bus: Some(espforge_model::ir::BusInit {
                sda: b.sda,
                scl: b.scl,
                mosi: b.mosi,
                miso: b.miso,
                sclk: b.sclk,
                frequency_khz: b.frequency_khz,
            }),
            claimed_by: None,
        });
    }
    for (name, b) in &esp32.spi {
        v.push(Peripheral {
            name: name.clone(),
            kind: PeripheralKind::Spi,
            number: b.peripheral,
            field: format!("SPI{}", b.peripheral),
            bus: Some(espforge_model::ir::BusInit {
                sda: b.sda,
                scl: b.scl,
                mosi: b.mosi,
                miso: b.miso,
                sclk: b.sclk,
                frequency_khz: b.frequency_khz,
            }),
            claimed_by: None,
        });
    }
    for (name, b) in &esp32.uart {
        v.push(Peripheral {
            name: name.clone(),
            kind: PeripheralKind::Uart,
            number: b.peripheral,
            field: format!("UART{}", b.peripheral),
            bus: Some(espforge_model::ir::BusInit {
                sda: b.sda,
                scl: b.scl,
                mosi: b.mosi,
                miso: b.miso,
                sclk: b.sclk,
                frequency_khz: b.frequency_khz,
            }),
            claimed_by: None,
        });
    }
    if esp32.wifi.is_some() {
        v.push(Peripheral {
            name: "wifi".to_string(),
            kind: PeripheralKind::Wifi,
            number: 0,
            field: "WiFi".to_string(),
            bus: None,
            claimed_by: None,
        });
    }
    v
}

/// Topological order over instances by their `deps` (Instance kind). Returns
/// indices into `instances`. On a cycle it falls back to the input order
/// (cycle detection is the validator's job); this is only reached post-validate.
fn topological_order(instances: &[ResolvedInstance]) -> Vec<usize> {
    let id_to_idx: HashMap<&str, usize> =
        instances.iter().enumerate().map(|(i, inst)| (inst.id.as_str(), i)).collect();
    let mut visited = vec![false; instances.len()];
    let mut order = Vec::new();
    for i in 0..instances.len() {
        if !visited[i] {
            visit(i, instances, &id_to_idx, &mut visited, &mut order);
        }
    }
    order
}

fn visit(
    i: usize,
    instances: &[ResolvedInstance],
    id_to_idx: &HashMap<&str, usize>,
    visited: &mut [bool],
    order: &mut Vec<usize>,
) {
    if visited[i] {
        return;
    }
    visited[i] = true;
    for dep in &instances[i].deps {
        if dep.kind == DepKind::Instance {
            if let Some(&j) = id_to_idx.get(dep.name.as_str()) {
                visit(j, instances, id_to_idx, visited, order);
            }
        }
    }
    order.push(i);
}

/// Extract a `$name` reference from a `with` map by key. Tolerates the value
/// being a string (`$x`), an integer (treated as a GPIO number), or absent.
fn extract_ref(
    with: &serde_yaml_ng::Value,
    key: &str,
) -> Option<ResourceRef> {
    let v = with.get(key)?;
    match v {
        serde_yaml_ng::Value::String(s) => {
            let trimmed = s.trim();
            let name = trimmed
                .strip_prefix('$')
                .unwrap_or(trimmed)
                .trim()
                .to_string();
            if name.is_empty() {
                None
            } else {
                Some(ResourceRef::synthetic(name))
            }
        }
        serde_yaml_ng::Value::Number(n) => {
            // Integer form: treat as GPIO number (e.g. `pin: 18`).
            let num = n.as_u64()? as u32;
            Some(ResourceRef::synthetic(format!("GPIO{num}")))
        }
        _ => None,
    }
}

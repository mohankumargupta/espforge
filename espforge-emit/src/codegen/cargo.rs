//! `Cargo.toml`, built with `toml_edit`.
//!
//! This replaces `Cargo.toml.tmpl`. The conditional dependency blocks are
//! ordinary Rust `if`s over [`ResolvedOptions`] instead of template guards, and
//! the comments that carry information (why `profile.dev` is optimized, what
//! `codegen-units = 1` buys, where to look for networking crates) are attached
//! as `toml_edit` decor so they survive serialization.
//!
//! Feature arrays keep the template's order. Cargo does not care, but the
//! dual-run gate compares parsed values and `toml` compares arrays in order, so
//! reproducing the order is what keeps that comparison meaningful.

use anyhow::Result;
use espforge_model::{Console, Meta, Runtime};
use espforge_options::ResolvedOptions;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value, value};

use super::chip::Chip;
use crate::runtime_source::{RUNTIME_VERSION, RuntimeSource};

/// The emitted `Cargo.toml`.
pub fn manifest(
    meta: &Meta,
    opts: &ResolvedOptions,
    runtime: &RuntimeSource,
    chip: &Chip,
) -> Result<String> {
    let name = meta.crate_name();
    let mut doc = DocumentMut::new();
    doc["package"] = package(meta, &name);
    doc["bin"] = Item::ArrayOfTables(bin(&name));
    doc["dependencies"] = dependencies(meta, opts, runtime, chip);
    doc["profile"] = profiles();
    Ok(doc.to_string())
}

fn package(meta: &Meta, name: &str) -> Item {
    let mut package = Table::new();
    package["name"] = value(name);
    package["version"] = value("0.1.0");
    package["edition"] = value("2024");
    package["rust-version"] = value("1.95");
    if let Some(description) = &meta.description {
        package["description"] = value(description.as_str());
    }
    Item::Table(package)
}

fn bin(name: &str) -> ArrayOfTables {
    let mut bin = Table::new();
    bin["name"] = value(name);
    bin["path"] = value("./src/bin/main.rs");
    let mut bins = ArrayOfTables::new();
    bins.push(bin);
    bins
}

fn dependencies(meta: &Meta, opts: &ResolvedOptions, runtime: &RuntimeSource, chip: &Chip) -> Item {
    let embassy = meta.runtime == Runtime::Embassy;
    let wifi = opts.is_on("wifi");
    let ble = opts.is_on("ble-trouble");
    let alloc = opts.is_on("alloc");
    let radio = wifi || ble;
    let chip_name = chip.name.as_str();

    let mut deps = Table::new();
    deps["espforge-runtime"] = Item::Value(runtime_dependency(runtime, embassy, wifi));
    deps["static_cell"] = value("2.1.1");
    deps["critical-section"] = value("1.2.0");
    deps["log"] = value("0.4.27");

    let mut hal = vec![chip_name];
    if opts.is_on("unstable-hal") {
        hal.push("unstable");
    }
    hal.push("log-04");
    deps["esp-hal"] = Item::Value(dep("~1.2.2", &hal));

    if embassy || radio {
        let mut features = Vec::new();
        if radio {
            features.push("esp-radio");
        }
        if alloc {
            features.push("esp-alloc");
        }
        if embassy {
            features.push("embassy");
        }
        features.push(chip_name);
        features.push("log-04");
        deps["esp-rtos"] = Item::Value(dep("0.4.0", &features));
    }

    deps["esp-bootloader-esp-idf"] = Item::Value(dep("0.6.0", &["log-04", chip_name]));
    deps["esp-println"] = Item::Value(dep_without_defaults(
        "0.18.0",
        &[chip_name, console(meta), "log-04"],
    ));
    deps["esp-backtrace"] = Item::Value(dep("0.20.0", &[chip_name, "panic-handler", "println"]));
    if alloc {
        deps["esp-alloc"] = Item::Value(dep("0.11.0", &[chip_name]));
    }

    if radio {
        deps["embedded-io"] = value("0.7.1");
        if embassy {
            deps["embedded-io-async"] = value("0.7.0");
            if wifi {
                deps["embassy-net"] = Item::Value(dep(
                    "0.9.1",
                    &["tcp", "udp", "dhcpv4", "medium-ethernet", "log"],
                ));
                if let Some(mut key) = deps.key_mut("embassy-net") {
                    key.leaf_decor_mut().set_prefix(
                        "# for more networking protocol support see https://crates.io/crates/edge-net\n",
                    );
                }
            }
        }
        if wifi {
            deps["smoltcp"] = Item::Value(dep_without_defaults(
                "0.14.0",
                &[
                    "medium-ethernet",
                    "multicast",
                    "proto-dhcpv4",
                    "proto-dns",
                    "proto-ipv4",
                    "socket-dns",
                    "socket-raw",
                    "socket-tcp",
                    "socket-udp",
                    "socket-icmp",
                    "log",
                ],
            ));
        }
        let mut features = vec![chip_name];
        if wifi {
            features.push("wifi");
        }
        if ble {
            features.push("ble");
        }
        if wifi && ble {
            features.push("coex");
        }
        if alloc {
            features.push("esp-alloc");
        }
        features.push("unstable");
        features.push("log-04");
        deps["esp-radio"] = Item::Value(dep("1.0.0-beta.1", &features));
        if ble {
            deps["trouble-host"] = Item::Value(dep("0.7.0", &["gatt"]));
            deps["bt-hci"] = value("0.9.0");
        }
    }

    if embassy {
        deps["embassy-executor"] = Item::Value(dep("0.10.0", &["log"]));
        deps["embassy-time"] = Item::Value(dep("0.5.0", &["log"]));
        deps["embassy-sync"] = value("0.8.0");
    }
    Item::Table(deps)
}

/// `[profile.dev]` / `[profile.release]`, with the template's comments.
fn profiles() -> Item {
    let mut dev = Table::new();
    dev.decor_mut().set_prefix(
        "# For fine tuning these settings, please refer to https://doc.rust-lang.org/cargo/reference/profiles.html\n",
    );
    dev["opt-level"] = value("s");
    if let Some(mut key) = dev.key_mut("opt-level") {
        key.leaf_decor_mut().set_prefix(
            "# The default debug profile is too slow and too big for resource-constrained devices.\n# Always build with some optimizations enabled.\n",
        );
    }

    let mut release = Table::new();
    let mut codegen_units = Value::from(1);
    codegen_units
        .decor_mut()
        .set_suffix("       # LLVM can perform better optimizations using a single thread");
    release["codegen-units"] = Item::Value(codegen_units);
    let mut debug = Value::from(2);
    debug
        .decor_mut()
        .set_suffix("               # prefer slower builds but better debugging experience");
    release["debug"] = Item::Value(debug);
    release["lto"] = value("fat");
    release["opt-level"] = value("s");

    let mut profile = Table::new();
    // `[profile.dev]` and `[profile.release]` are the headers; the parent is
    // implicit, as it was in the template.
    profile.set_implicit(true);
    profile["dev"] = Item::Table(dev);
    profile["release"] = Item::Table(release);
    Item::Table(profile)
}

/// `espforge-runtime = { path?, version, features? }`.
fn runtime_dependency(source: &RuntimeSource, embassy: bool, wifi: bool) -> Value {
    let mut table = InlineTable::new();
    if let RuntimeSource::Path(path) = source {
        table.insert("path", Value::from(path.replace('\\', "/")));
    }
    table.insert("version", Value::from(RUNTIME_VERSION));
    if wifi {
        table.insert("features", Value::Array(array(&["embassy", "wifi"])));
    } else if embassy {
        table.insert("features", Value::Array(array(&["embassy"])));
    }
    Value::InlineTable(table)
}

/// `{ version = "...", features = [...] }`.
fn dep(version: &str, features: &[&str]) -> Value {
    let mut table = InlineTable::new();
    table.insert("version", Value::from(version));
    table.insert("features", Value::Array(array(features)));
    Value::InlineTable(table)
}

/// The same, with `default-features = false`.
fn dep_without_defaults(version: &str, features: &[&str]) -> Value {
    let mut table = InlineTable::new();
    table.insert("version", Value::from(version));
    table.insert("default-features", Value::from(false));
    table.insert("features", Value::Array(array(features)));
    Value::InlineTable(table)
}

fn array(items: &[&str]) -> Array {
    let mut array = Array::new();
    for item in items {
        array.push(*item);
    }
    array
}

fn console(meta: &Meta) -> &'static str {
    match meta.console {
        Console::Uart => "uart",
        Console::JtagSerial => "jtag-serial",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::{case, chip};
    use espforge_model::Runtime;

    fn cargo(defaults: bool, flags: &[(&str, &str)], runtime: Runtime) -> String {
        let case = case("esp32c3", runtime, defaults, flags, &[]);
        manifest(
            &case.meta,
            &case.opts,
            &RuntimeSource::Published,
            &chip("esp32c3"),
        )
        .unwrap()
    }

    fn parse(text: &str) -> toml_edit::DocumentMut {
        text.parse().unwrap_or_else(|e| panic!("{e}\n{text}"))
    }

    /// A dependency's rendered value, without the key's leading decor.
    fn dependency(doc: &toml_edit::DocumentMut, name: &str) -> String {
        doc["dependencies"][name]
            .as_value()
            .unwrap_or_else(|| panic!("{name} is not a value"))
            .to_string()
            .trim()
            .to_string()
    }

    #[test]
    fn the_blocking_project_has_no_async_or_radio_dependencies() {
        let doc = parse(&cargo(true, &[], Runtime::Blocking));
        let deps = doc["dependencies"].as_table().unwrap();
        assert_eq!(
            dependency(&doc, "espforge-runtime"),
            "{ version = \"0.6.0\" }"
        );
        assert!(deps.get("embassy-executor").is_none());
        assert!(deps.get("esp-rtos").is_none());
        assert!(deps.get("esp-radio").is_none());
        assert_eq!(
            features(&doc, "esp-hal"),
            vec!["esp32c3", "unstable", "log-04"]
        );
        assert_eq!(
            features(&doc, "esp-println"),
            vec!["esp32c3", "uart", "log-04"]
        );
    }

    #[test]
    fn the_embassy_project_adds_the_executor_rtos_and_sync_crates() {
        let doc = parse(&cargo(true, &[], Runtime::Embassy));
        assert_eq!(
            features(&doc, "esp-rtos"),
            vec!["esp-alloc", "embassy", "esp32c3", "log-04"]
        );
        assert!(doc["dependencies"].get("embassy-sync").is_some());
        assert_eq!(
            dependency(&doc, "espforge-runtime"),
            "{ version = \"0.6.0\", features = [\"embassy\"] }"
        );
    }

    #[test]
    fn wifi_brings_the_networking_stack() {
        let case = case("esp32c3", Runtime::Embassy, true, &[], &["wifi"]);
        let doc = parse(
            &manifest(
                &case.meta,
                &case.opts,
                &RuntimeSource::Published,
                &chip("esp32c3"),
            )
            .unwrap(),
        );
        assert_eq!(
            features(&doc, "esp-radio"),
            vec!["esp32c3", "wifi", "esp-alloc", "unstable", "log-04"]
        );
        assert!(doc["dependencies"].get("smoltcp").is_some());
        assert_eq!(
            features(&doc, "embassy-net"),
            vec!["tcp", "udp", "dhcpv4", "medium-ethernet", "log"]
        );
    }

    #[test]
    fn ble_only_adds_trouble_and_keeps_coex_off() {
        let case = case(
            "esp32c3",
            Runtime::Embassy,
            true,
            &[("ble-trouble", "true")],
            &[],
        );
        let doc = parse(
            &manifest(
                &case.meta,
                &case.opts,
                &RuntimeSource::Published,
                &chip("esp32c3"),
            )
            .unwrap(),
        );
        assert!(doc["dependencies"].get("trouble-host").is_some());
        assert_eq!(
            features(&doc, "esp-radio"),
            vec!["esp32c3", "ble", "esp-alloc", "unstable", "log-04"]
        );
        assert!(doc["dependencies"].get("embassy-net").is_none());
    }

    #[test]
    fn the_description_is_written_as_a_string_and_defaults_can_be_off() {
        let case = case("esp32s3", Runtime::Blocking, false, &[], &[]);
        let doc = parse(
            &manifest(
                &case.meta,
                &case.opts,
                &RuntimeSource::Path("..\\rt".into()),
                &chip("esp32s3"),
            )
            .unwrap(),
        );
        assert_eq!(doc["package"]["description"].as_str(), Some("demo"));
        assert_eq!(
            dependency(&doc, "espforge-runtime"),
            "{ path = \"../rt\", version = \"0.6.0\" }"
        );
        // with defaults off, neither alloc nor unstable-hal is implied
        assert!(doc["dependencies"].get("esp-alloc").is_none());
        assert_eq!(features(&doc, "esp-hal"), vec!["esp32s3", "log-04"]);
    }

    #[test]
    fn the_profile_comments_carry_through() {
        let text = cargo(true, &[], Runtime::Blocking);
        assert!(
            text.contains("# The default debug profile is too slow"),
            "{text}"
        );
        assert!(
            text.contains("# LLVM can perform better optimizations"),
            "{text}"
        );
        assert!(text.contains("opt-level = \"s\""), "{text}");
    }

    fn features(doc: &toml_edit::DocumentMut, dep: &str) -> Vec<String> {
        doc["dependencies"][dep]["features"]
            .as_array()
            .unwrap_or_else(|| panic!("{dep} has no features"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }
}

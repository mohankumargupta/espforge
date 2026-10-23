//! The minijinja context every template is rendered with.
//!
//! | name           | value |
//! |----------------|-------|
//! | `project_name` | slugged crate name (`hello_world`) |
//! | `project`      | `{ name, description }` (raw, as written) |
//! | `chip`         | `{ name, rust_target, riscv, xtensa }` from esp-metadata |
//! | `board`        | Wokwi board id for the chip |
//! | `serialMonitor`| `"$serialMonitor"` (Wokwi's serial monitor part) |
//! | `console`      | `"uart"` or `"jtag-serial"` |
//! | `runtime`      | `"blocking"` or `"embassy"` |
//! | `embassy`      | `true` for the embassy runtime |
//! | `opt`          | every resolved option, incl. derived/implied flags; use `opt["probe-rs"]` for hyphenated names |
//! | `log`          | always `true` (logging is `log` only) |
//! | `runtime_dep`  | TOML inline table for the `espforge-runtime` dependency |
//! | `leds`         | list of `{ name, pin, active_low }`, one per `<led>` component |
//! | `buttons`      | list of `{ name, pin, pull_up }`, one per `<button>` component |
//! | `i2cs`         | list of `{ name, peripheral, sda, scl, khz }`, one per `<i2c_component>` |
//! | `spi_buses`    | list of `{ id, peripheral, sclk, mosi, miso, khz, mode }`, one used `<spi>` |
//! | `spis`         | list of `{ name, bus, cs }`, one per `<spi_component>`, by component id |
//! | `uarts`        | list of `{ name, peripheral, tx, rx, baud, parity, stop_bits }`, one per `<uart_component>` |
//! | `<led name>`   | each led again, by its component id (`red_led.pin` in diagram.json) |
//! | `<button name>`| each button again, by its component id (`button.pin` in diagram.json) |
//! | `wifi`         | `{ ssid, password }` when `<wifi>` is in `# services`, else none (falsy) |

use anyhow::{Context as _, Result};
use espforge_model::{Console, Meta, Runtime};
// Spec structs live in `espforge-model` (step 1b); re-exported here so
// downstream imports keep working until step 1f removes this shim.
pub use espforge_model::hardware::{
    ButtonSpec, Hardware, I2cSpec, LedSpec, SpiBusSpec, SpiSpec, UartSpec, WifiSpec,
};
use espforge_options::{OptValue, ResolvedOptions};
use minijinja::{Value, context};
use std::collections::BTreeMap;

/// Version of `espforge-runtime` written into generated manifests.
pub const RUNTIME_VERSION: &str = "0.1.0";

#[derive(Debug, Clone)]
pub enum RuntimeSource {
    /// Use the published crate from crates.io.
    Published,
    /// Path to the `espforge-runtime` crate directory, as it should appear
    /// in the generated Cargo.toml.
    Path(String),
}

/// Wokwi board id for a chip (moved out of the old `diagram.json` if-chain).
pub fn wokwi_board(chip: &str) -> &'static str {
    match chip {
        "esp32c3" => "board-esp32-c3-devkitm-1",
        "esp32c6" => "board-esp32-c6-devkitc-1",
        "esp32h2" => "board-esp32-h2-devkitm-1",
        "esp32s2" => "board-esp32-s2-devkitm-1",
        "esp32s3" => "board-esp32-s3-devkitc-1",
        // "esp32" and anything Wokwi has no dedicated board for.
        _ => "board-esp32-devkit-c-v4",
    }
}

/// The `espforge-runtime` dependency as a TOML inline table. Wi-Fi implies
/// the embassy runtime.
pub fn runtime_dep(source: &RuntimeSource, embassy: bool, wifi: bool) -> String {
    let feats = if wifi {
        ", features = [\"embassy\", \"wifi\"]"
    } else if embassy {
        ", features = [\"embassy\"]"
    } else {
        ""
    };
    match source {
        RuntimeSource::Published => {
            format!("{{ version = \"{RUNTIME_VERSION}\"{feats} }}")
        }
        RuntimeSource::Path(p) => {
            let p = p.replace('\\', "/");
            format!("{{ path = \"{p}\", version = \"{RUNTIME_VERSION}\"{feats} }}")
        }
    }
}

fn opt_value(v: &OptValue) -> Value {
    match v {
        OptValue::Bool(b) => Value::from(*b),
        OptValue::Str(s) => Value::from(s.as_str()),
        OptValue::List(l) => Value::from(l.clone()),
    }
}

/// Context for a project with no hardware.
pub fn build_context(
    meta: &Meta,
    opts: &ResolvedOptions,
    runtime_source: &RuntimeSource,
) -> Result<Value> {
    build_context_with(meta, opts, runtime_source, &Hardware::default())
}

pub fn build_context_with(
    meta: &Meta,
    opts: &ResolvedOptions,
    runtime_source: &RuntimeSource,
    hardware: &Hardware,
) -> Result<Value> {
    let chip = esp_metadata_generated::Chip::iter()
        .find(|c| c.name() == meta.chip)
        .with_context(|| format!("unknown chip `{}`", meta.chip))?;
    let embassy = meta.runtime == Runtime::Embassy;

    let opt: BTreeMap<String, Value> = opts
        .iter()
        .map(|(name, r)| (name.clone(), opt_value(&r.value)))
        .collect();

    let led_value = |l: &LedSpec| {
        context! {
            name => l.name.clone(),
            pin => l.pin,
            active_low => l.active_low,
        }
    };

    let button_value = |b: &ButtonSpec| {
        context! {
            name => b.name.clone(),
            pin => b.pin,
            pull_up => b.pull_up,
        }
    };

    let i2c_value = |b: &I2cSpec| {
        context! {
            name => b.name.clone(),
            peripheral => b.peripheral.clone(),
            sda => b.sda,
            scl => b.scl,
            khz => b.khz,
        }
    };

    let spi_bus_value = |b: &SpiBusSpec| {
        context! {
            id => b.id.clone(),
            peripheral => b.peripheral.clone(),
            sclk => b.sclk,
            mosi => b.mosi,
            miso => b.miso,
            khz => b.khz,
            mode => b.mode,
        }
    };

    let spi_value = |s: &SpiSpec| {
        context! {
            name => s.name.clone(),
            bus => s.bus.clone(),
            cs => s.cs,
        }
    };

    let uart_value = |u: &UartSpec| {
        context! {
            name => u.name.clone(),
            peripheral => u.peripheral.clone(),
            tx => u.tx,
            rx => u.rx,
            baud => u.baud,
            parity => u.parity.clone(),
            stop_bits => u.stop_bits.clone(),
        }
    };

    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    // Hardware first, so the fixed names below always win a collision.
    for led in &hardware.leds {
        map.insert(led.name.clone(), led_value(led));
    }
    for button in &hardware.buttons {
        map.insert(button.name.clone(), button_value(button));
    }
    for i2c in &hardware.i2cs {
        map.insert(i2c.name.clone(), i2c_value(i2c));
    }
    for spi in &hardware.spis {
        map.insert(spi.name.clone(), spi_value(spi));
    }
    for uart in &hardware.uarts {
        map.insert(uart.name.clone(), uart_value(uart));
    }
    map.insert(
        "leds".into(),
        Value::from(hardware.leds.iter().map(led_value).collect::<Vec<_>>()),
    );
    map.insert(
        "buttons".into(),
        Value::from(
            hardware
                .buttons
                .iter()
                .map(button_value)
                .collect::<Vec<_>>(),
        ),
    );
    map.insert(
        "i2cs".into(),
        Value::from(hardware.i2cs.iter().map(i2c_value).collect::<Vec<_>>()),
    );
    map.insert(
        "spi_buses".into(),
        Value::from(
            hardware
                .spi_buses
                .iter()
                .map(spi_bus_value)
                .collect::<Vec<_>>(),
        ),
    );
    map.insert(
        "spis".into(),
        Value::from(hardware.spis.iter().map(spi_value).collect::<Vec<_>>()),
    );
    map.insert(
        "uarts".into(),
        Value::from(hardware.uarts.iter().map(uart_value).collect::<Vec<_>>()),
    );
    map.insert(
        "wifi".into(),
        match &hardware.wifi {
            Some(w) => context! {
                ssid => w.ssid.clone(),
                password => w.password.clone(),
            },
            // Always defined (strict mode), but falsy.
            None => Value::from(()),
        },
    );
    map.insert("project_name".into(), Value::from(meta.crate_name()));
    map.insert(
        "project".into(),
        context! {
            name => meta.name.clone(),
            description => meta.description.clone(),
        },
    );
    map.insert(
        "chip".into(),
        context! {
            name => chip.name(),
            rust_target => chip.target(),
            riscv => !chip.is_xtensa(),
            xtensa => chip.is_xtensa(),
        },
    );
    map.insert("board".into(), Value::from(wokwi_board(chip.name())));
    map.insert("serialMonitor".into(), Value::from("$serialMonitor"));
    map.insert(
        "console".into(),
        Value::from(match meta.console {
            Console::Uart => "uart",
            Console::JtagSerial => "jtag-serial",
        }),
    );
    map.insert(
        "runtime".into(),
        Value::from(match meta.runtime {
            Runtime::Blocking => "blocking",
            Runtime::Embassy => "embassy",
        }),
    );
    map.insert("embassy".into(), Value::from(embassy));
    map.insert("opt".into(), Value::from(opt));
    map.insert("log".into(), Value::from(true));
    map.insert(
        "runtime_dep".into(),
        Value::from(runtime_dep(
            runtime_source,
            embassy,
            hardware.wifi.is_some(),
        )),
    );
    Ok(Value::from(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_options::{Answers, DerivedInputs};

    pub fn meta(runtime: Runtime, chip: &str) -> Meta {
        Meta {
            name: "hello world".into(),
            description: Some("demo".into()),
            chip: chip.into(),
            runtime,
            console: Console::Uart,
        }
    }

    fn ctx(runtime: Runtime, source: RuntimeSource) -> Value {
        let m = meta(runtime, "esp32c3");
        let opts = espforge_options::resolve(&m, &Answers::empty(0..0), &DerivedInputs::default())
            .unwrap();
        build_context(&m, &opts, &source).unwrap()
    }

    fn get(v: &Value, path: &str) -> Value {
        path.split('.').fold(v.clone(), |acc, k| {
            acc.get_attr(k).unwrap_or_else(|_| panic!("missing {k}"))
        })
    }

    #[test]
    fn helloworld_context_has_every_documented_name() {
        let c = ctx(Runtime::Blocking, RuntimeSource::Published);
        assert_eq!(get(&c, "project_name").as_str(), Some("hello_world"));
        assert_eq!(get(&c, "project.name").as_str(), Some("hello world"));
        assert_eq!(get(&c, "project.description").as_str(), Some("demo"));
        assert_eq!(get(&c, "chip.name").as_str(), Some("esp32c3"));
        assert!(get(&c, "chip.riscv").is_true());
        assert!(!get(&c, "chip.xtensa").is_true());
        assert!(
            get(&c, "chip.rust_target")
                .as_str()
                .unwrap()
                .starts_with("riscv32")
        );
        assert_eq!(get(&c, "board").as_str(), Some("board-esp32-c3-devkitm-1"));
        assert_eq!(get(&c, "serialMonitor").as_str(), Some("$serialMonitor"));
        assert_eq!(get(&c, "console").as_str(), Some("uart"));
        assert_eq!(get(&c, "runtime").as_str(), Some("blocking"));
        assert!(!get(&c, "embassy").is_true());
        assert!(get(&c, "log").is_true());
        // alloc and unstable-hal are on by default (what espforge always asked esp-generate for)
        assert!(get(&c, "opt.alloc").is_true());
        assert!(get(&c, "opt.unstable-hal").is_true());
        assert!(!get(&c, "opt.keep-zig-source").is_true());
        // no hardware: an empty (but defined) list, so templates can loop over it
        assert_eq!(get(&c, "leds").len(), Some(0));
        // no wifi service: defined but falsy
        assert!(!get(&c, "wifi").is_true());
    }

    #[test]
    fn leds_are_reachable_by_list_and_by_name() {
        let m = meta(Runtime::Blocking, "esp32c3");
        let opts = espforge_options::resolve(&m, &Answers::empty(0..0), &DerivedInputs::default())
            .unwrap();
        let hw = Hardware {
            leds: vec![LedSpec {
                name: "red_led".into(),
                pin: 18,
                active_low: false,
            }],
            ..Default::default()
        };
        let c = build_context_with(&m, &opts, &RuntimeSource::Published, &hw).unwrap();
        assert_eq!(get(&c, "leds").len(), Some(1));
        assert_eq!(get(&c, "red_led.name").as_str(), Some("red_led"));
        let env = minijinja::Environment::new();
        assert_eq!(env.render_str("{{ red_led.pin }}", &c).unwrap(), "18");
        assert_eq!(
            env.render_str("{{ red_led.active_low }}", &c).unwrap(),
            "false"
        );
    }

    #[test]
    fn the_wifi_service_reaches_the_context_and_the_runtime_dependency() {
        let m = meta(Runtime::Embassy, "esp32c3");
        let opts = espforge_options::resolve(&m, &Answers::empty(0..0), &DerivedInputs::default())
            .unwrap();
        let hw = Hardware {
            wifi: Some(WifiSpec {
                ssid: "Wokwi-GUEST".into(),
                password: String::new(),
            }),
            ..Default::default()
        };
        let c = build_context_with(&m, &opts, &RuntimeSource::Published, &hw).unwrap();
        assert!(get(&c, "wifi").is_true());
        assert_eq!(get(&c, "wifi.ssid").as_str(), Some("Wokwi-GUEST"));
        assert_eq!(get(&c, "wifi.password").as_str(), Some(""));
        assert_eq!(
            get(&c, "runtime_dep").as_str(),
            Some("{ version = \"0.1.0\", features = [\"embassy\", \"wifi\"] }")
        );
    }

    #[test]
    fn spi_devices_are_reachable_by_list_and_by_name() {
        let m = meta(Runtime::Blocking, "esp32c3");
        let opts = espforge_options::resolve(&m, &Answers::empty(0..0), &DerivedInputs::default())
            .unwrap();
        let hw = Hardware {
            spi_buses: vec![SpiBusSpec {
                id: "spi2".into(),
                peripheral: "SPI2".into(),
                sclk: 8,
                mosi: 7,
                miso: 6,
                khz: 100,
                mode: 0,
            }],
            spis: vec![SpiSpec {
                name: "spi_tester".into(),
                bus: "spi2".into(),
                cs: 9,
            }],
            ..Default::default()
        };
        let c = build_context_with(&m, &opts, &RuntimeSource::Published, &hw).unwrap();
        assert_eq!(get(&c, "spis").len(), Some(1));
        assert_eq!(get(&c, "spi_buses").len(), Some(1));
        assert_eq!(get(&c, "spi_tester.bus").as_str(), Some("spi2"));
        let env = minijinja::Environment::new();
        assert_eq!(env.render_str("{{ spi_tester.cs }}", &c).unwrap(), "9");
        assert_eq!(
            env.render_str("{{ spi_buses[0].peripheral }}", &c).unwrap(),
            "SPI2"
        );
    }

    #[test]
    fn hyphenated_options_are_reachable_by_index() {
        let c = ctx(Runtime::Blocking, RuntimeSource::Published);
        let env = minijinja::Environment::new();
        let out = env.render_str("{{ opt['probe-rs'] }}", &c).unwrap();
        assert_eq!(out, "false");
    }

    #[test]
    fn embassy_runtime_sets_flags_and_runtime_dep() {
        let c = ctx(Runtime::Embassy, RuntimeSource::Published);
        assert!(get(&c, "embassy").is_true());
        assert!(get(&c, "opt.embassy").is_true());
        // implied by embassy's `requires`
        assert!(get(&c, "opt.unstable-hal").is_true());
        assert_eq!(
            get(&c, "runtime_dep").as_str(),
            Some("{ version = \"0.1.0\", features = [\"embassy\"] }")
        );
    }

    #[test]
    fn runtime_dep_path_form() {
        assert_eq!(
            runtime_dep(&RuntimeSource::Path("..\\rt".into()), false, false),
            "{ path = \"../rt\", version = \"0.1.0\" }"
        );
    }

    #[test]
    fn board_table_matches_the_old_diagram_chain() {
        assert_eq!(wokwi_board("esp32"), "board-esp32-devkit-c-v4");
        assert_eq!(wokwi_board("esp32s3"), "board-esp32-s3-devkitc-1");
        assert_eq!(wokwi_board("esp32c61"), "board-esp32-devkit-c-v4");
    }

    #[test]
    fn unknown_chip_is_an_error() {
        let m = meta(Runtime::Blocking, "esp99");
        let opts = ResolvedOptions::default();
        assert!(build_context(&m, &opts, &RuntimeSource::Published).is_err());
    }
}

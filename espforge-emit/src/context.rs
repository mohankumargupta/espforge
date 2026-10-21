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

use anyhow::{Context as _, Result};
use espforge_model::{Console, Meta, Runtime};
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

/// The `espforge-runtime` dependency as a TOML inline table.
pub fn runtime_dep(source: &RuntimeSource, embassy: bool) -> String {
    let feats = if embassy {
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

pub fn build_context(
    meta: &Meta,
    opts: &ResolvedOptions,
    runtime_source: &RuntimeSource,
) -> Result<Value> {
    let chip = esp_metadata_generated::Chip::iter()
        .find(|c| c.name() == meta.chip)
        .with_context(|| format!("unknown chip `{}`", meta.chip))?;
    let embassy = meta.runtime == Runtime::Embassy;

    let opt: BTreeMap<String, Value> = opts
        .iter()
        .map(|(name, r)| (name.clone(), opt_value(&r.value)))
        .collect();

    Ok(context! {
        project_name => meta.crate_name(),
        project => context! {
            name => meta.name.clone(),
            description => meta.description.clone(),
        },
        chip => context! {
            name => chip.name(),
            rust_target => chip.target(),
            riscv => !chip.is_xtensa(),
            xtensa => chip.is_xtensa(),
        },
        board => wokwi_board(chip.name()),
        serialMonitor => "$serialMonitor",
        console => match meta.console {
            Console::Uart => "uart",
            Console::JtagSerial => "jtag-serial",
        },
        runtime => match meta.runtime {
            Runtime::Blocking => "blocking",
            Runtime::Embassy => "embassy",
        },
        embassy => embassy,
        opt => Value::from(opt),
        log => true,
        runtime_dep => runtime_dep(runtime_source, embassy),
    })
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
            runtime_dep(&RuntimeSource::Path("..\\rt".into()), false),
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

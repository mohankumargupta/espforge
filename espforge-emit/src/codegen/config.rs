//! `.cargo/config.toml`, `.cargo/esp-config.toml` and `rust-toolchain.toml`.
//!
//! These replace the three template files of the same names. The two shipped
//! as `.cargo/**` plus the toolchain file are seed-once, so a user who edited
//! them keeps their version.
//!
//! `esp-config.toml` is static text; it is here so one module owns the whole
//! `.cargo` directory. The comments in `config.toml` explain a real choice
//! (frame pointers cost performance but give backtraces), so they are kept as
//! decor above the key they belong to rather than dropped with the template.

use anyhow::Result;
use espforge_elements::ResolvedOptions;
use toml_edit::{Array, DocumentMut, Item, Table, value};

use super::chip::Chip;

/// The emitted `.cargo/config.toml`.
pub fn cargo_config(opts: &ResolvedOptions, chip: &Chip) -> Result<String> {
    let mut doc = DocumentMut::new();
    doc["include"] = value(array(&["esp-config.toml"]));
    if let Some(mut key) = doc.as_table_mut().key_mut("include") {
        key.leaf_decor_mut().set_prefix(
            "# Keep compilation settings in this file and user/runtime settings in `esp-config.toml`.\n",
        );
    }

    let mut target = Table::new();
    // `[target.<triple>]` is the header; the parent stays implicit.
    target.set_implicit(true);
    let mut entry = Table::new();
    entry["runner"] = value(runner(opts, chip));
    target[chip.rust_target.as_str()] = Item::Table(entry);
    doc["target"] = Item::Table(target);

    let mut build = Table::new();
    build["rustflags"] = value(flags(opts, chip));
    if chip.riscv() {
        if let Some(mut key) = build.key_mut("rustflags") {
            key.leaf_decor_mut().set_prefix(
                "# Required to obtain backtraces (e.g. when using the \"esp-backtrace\" crate.)\n\
                 # NOTE: May negatively impact performance of produced code\n",
            );
        }
    }
    build["target"] = value(chip.rust_target.as_str());
    doc["build"] = Item::Table(build);

    let mut unstable = Table::new();
    unstable["build-std"] = value(array(if opts.is_on("alloc") {
        &["alloc", "core"]
    } else {
        &["core"]
    }));
    doc["unstable"] = Item::Table(unstable);

    Ok(doc.to_string())
}

fn runner(opts: &ResolvedOptions, chip: &Chip) -> String {
    if opts.is_on("probe-rs") {
        format!(
            "probe-rs run --chip={} --preverify --always-print-stacktrace --no-location",
            chip.name
        )
    } else {
        format!("espflash flash --monitor --chip {}", chip.name)
    }
}

fn flags(opts: &ResolvedOptions, chip: &Chip) -> Array {
    let mut flags = if chip.xtensa() {
        array(&["-C", "link-arg=-nostartfiles"])
    } else {
        array(&["-C", "force-frame-pointers"])
    };
    if opts.is_on("stack-smashing-protection") {
        flags.push("-Z");
        flags.push("stack-protector=all");
    }
    flags
}

/// The emitted `rust-toolchain.toml`.
pub fn toolchain(chip: &Chip) -> Result<String> {
    let mut toolchain = Table::new();
    if chip.riscv() {
        toolchain["channel"] = value("stable");
        toolchain["components"] = value(array(&["rust-src"]));
        toolchain["targets"] = value(array(&[chip.rust_target.as_str()]));
    } else {
        // espup's `esp` channel carries the Xtensa fork of the toolchain.
        toolchain["channel"] = value("esp");
    }
    let mut doc = DocumentMut::new();
    doc["toolchain"] = Item::Table(toolchain);
    Ok(doc.to_string())
}

/// The emitted `.cargo/esp-config.toml`: static, seed-once.
pub fn esp_config() -> &'static str {
    "# Keep user/runtime configuration values in this file.\n[env]\nESP_LOG=\"info\"\n"
}

fn array(items: &[&str]) -> Array {
    let mut array = Array::new();
    for item in items {
        array.push(*item);
    }
    array
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::test_support::{case, chip};
    use espforge_elements::Runtime;
    use toml_edit::DocumentMut;

    fn parse(text: &str) -> DocumentMut {
        text.parse().unwrap_or_else(|e| panic!("{e}\n{text}"))
    }

    fn config(chip_name: &str, flags: &[(&str, &str)]) -> DocumentMut {
        let case = case(chip_name, Runtime::Blocking, true, flags, &[]);
        parse(&cargo_config(&case.opts, &chip(chip_name)).unwrap())
    }

    #[test]
    fn a_riscv_project_gets_espflash_and_frame_pointers() {
        let doc = config("esp32c3", &[]);
        assert_eq!(
            doc["target"]["riscv32imc-unknown-none-elf"]["runner"].as_str(),
            Some("espflash flash --monitor --chip esp32c3")
        );
        assert_eq!(
            doc["build"]["target"].as_str(),
            Some("riscv32imc-unknown-none-elf")
        );
        let flags: Vec<&str> = doc["build"]["rustflags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(flags, vec!["-C", "force-frame-pointers"]);
        assert_eq!(doc["include"][0].as_str(), Some("esp-config.toml"));
    }

    #[test]
    fn an_xtensa_project_gets_the_no_startfiles_link_argument() {
        let doc = config("esp32s3", &[]);
        let flags: Vec<&str> = doc["build"]["rustflags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(flags, vec!["-C", "link-arg=-nostartfiles"]);
    }

    #[test]
    fn probe_rs_replaces_the_runner_and_stack_protection_appends_flags() {
        let doc = config(
            "esp32c3",
            &[("probe-rs", "true"), ("stack-smashing-protection", "true")],
        );
        assert_eq!(
            doc["target"]["riscv32imc-unknown-none-elf"]["runner"].as_str(),
            Some("probe-rs run --chip=esp32c3 --preverify --always-print-stacktrace --no-location")
        );
        let flags: Vec<&str> = doc["build"]["rustflags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            flags,
            vec!["-C", "force-frame-pointers", "-Z", "stack-protector=all"]
        );
    }

    #[test]
    fn build_std_follows_the_alloc_option() {
        let off = case("esp32c3", Runtime::Blocking, false, &[], &[]);
        let doc = parse(&cargo_config(&off.opts, &chip("esp32c3")).unwrap());
        let std: Vec<&str> = doc["unstable"]["build-std"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(std, vec!["core"]);
    }

    #[test]
    fn the_toolchain_is_stable_plus_rust_src_on_riscv_and_esp_on_xtensa() {
        let riscv = parse(&toolchain(&chip("esp32c3")).unwrap());
        assert_eq!(riscv["toolchain"]["channel"].as_str(), Some("stable"));
        assert_eq!(
            riscv["toolchain"]["components"][0].as_str(),
            Some("rust-src")
        );
        assert_eq!(
            riscv["toolchain"]["targets"][0].as_str(),
            Some("riscv32imc-unknown-none-elf")
        );

        let xtensa = parse(&toolchain(&chip("esp32s3")).unwrap());
        assert_eq!(xtensa["toolchain"]["channel"].as_str(), Some("esp"));
        assert!(xtensa["toolchain"].get("components").is_none());
    }

    #[test]
    fn esp_config_is_the_static_file() {
        assert!(esp_config().contains("ESP_LOG=\"info\""));
    }
}

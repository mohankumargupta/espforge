//! Check 2 of the slice gate: the project we generate actually compiles.
//!
//! Byte-identical goldens prove the emitter reproduces the legacy tree; this
//! proves the tree is a real crate. It is `#[ignore]`d because it spawns cargo
//! and needs the generated project's target installed:
//!
//! ```text
//! rustup target add riscv32imc-unknown-none-elf
//! cargo test -p espforge --test generated_projects_compile -- --ignored
//! ```
//!
//! The generated `Cargo.toml` points at this checkout's `espforge-runtime`
//! (via `--runtime-path`, here passed straight to `generate_with`), so the
//! runtime is compiled by this test too.

use std::path::{Path, PathBuf};
use std::process::Command;

use espforge_core::answers::Overrides;
use espforge_core::project::generate_with;
use espforge_core::source::resolve_source;
use espforge_elements::Runtime;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("espforge/ lives inside the workspace")
        .to_path_buf()
}

/// Generate one cell of an example into a scratch dir and `cargo check` it,
/// with the runtime from this checkout and the shared target dir.
fn check_example(case: &Case, runtime: &Path, target_dir: &Path) {
    let Case {
        example,
        chip,
        runtime: async_model,
        inline,
    } = case;
    let scratch = tempfile::tempdir().expect("create scratch dir");
    let out = scratch.path().join("out");
    let mut source = resolve_source(None, Some(example), Path::new(".")).expect("embedded example");
    if let Some(text) = inline {
        source.name = format!("inline:{example}:{chip}");
        source.text = (*text).to_string();
    }
    let overrides = Overrides {
        chip: Some(chip.to_string()),
        runtime: Some(*async_model),
        ..Default::default()
    };
    generate_with(&source, &out, &overrides, Some(runtime.to_path_buf()))
        .unwrap_or_else(|e| panic!("generate {example} ({chip}): {e:?}"));

    let output = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(&out)
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .expect("run cargo check");
    assert!(
        output.status.success(),
        "generated {example} ({chip}) does not compile:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// One generated project to compile: an example, a chip and a runtime.
struct Case {
    example: &'static str,
    chip: &'static str,
    runtime: Runtime,
    /// A spec to compile instead of the example's own text. The example still
    /// supplies the app logic, so only the hardware changes.
    inline: Option<&'static str>,
}

/// The cells this test compiles, all on `esp32c3`: the only target this
/// checkout installs. `esp32s3` is what would prove a non-default instance
/// (`I2C1` does not exist on the C3, `SPI3`/`UART2` are chip-dependent), but
/// the Xtensa toolchain is out of scope here — see `buses.md` §11.
const CASES: &[Case] = &[
    Case {
        example: "01.Basics/helloworld",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "01.Basics/blink",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "02.Digital/button",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "02.Digital/button_async",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "04.Communication/i2c_example",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    // The raw `I2cBus` shape: an unaddressed component is the bus itself, so
    // the scanner is a different emitted field from every addressed one.
    Case {
        example: "04.Communication/i2c_scanner",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "04.Communication/i2c_example_async",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "04.Communication/spi_example",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "04.Communication/spi_example_async",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "04.Communication/uart_example",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "04.Communication/uart_example_async",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "05.Networking/wifi_station",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "05.Networking/http_client",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    // Two HTTP slots: the `::<2>` the recipe bakes into the types is a const
    // argument of real types, not just emitted text.
    Case {
        example: "05.Networking/http_client",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: Some(
            "# espforge\n\
             \x20 <project name=\"http_two_slots\" />\n\
             \x20 <chip type=\"esp32c3\" />\n\
             \x20 <runtime type=\"embassy\" />\n\
             # peripherals\n\
             \x20 <wifi id=\"radio\" />\n\
             # devices\n\
             # services\n\
             \x20 <wifi_station id=\"home\" wifi=\"$radio\" ssid=\"Wokwi-GUEST\" auth=\"open\" />\n\
             \x20 <http_client id=\"client\" network=\"$home\" connections=\"2\" />\n",
        ),
    },
    Case {
        example: "05.Networking/https_client",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "05.Networking/ws_client",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "05.Networking/wss_client",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "05.Networking/ntp",
        chip: "esp32c3",
        runtime: Runtime::Embassy,
        inline: None,
    },
    Case {
        example: "06.Displays/ssd1306",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    Case {
        example: "06.Displays/ili9341",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: None,
    },
    // Multi-drop: a display and a sensor on one `<i2c>`, each at its own
    // address. It is the only case that takes two devices on one controller
    // through `rustc`; the display's app supplies the logic.
    Case {
        example: "06.Displays/ssd1306",
        chip: "esp32c3",
        runtime: Runtime::Blocking,
        inline: Some(
            "# espforge\n\
             \x20 <project name=\"i2c_multidrop\" />\n\
             \x20 <chip type=\"esp32c3\" />\n\
             \x20 <runtime type=\"blocking\" />\n\
             # peripherals\n\
             \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" frequency=\"400kHz\" />\n\
             # devices\n\
             \x20 <ssd1306 id=\"oled\" bus=\"$i2c0\" address=\"0x3C\" size=\"128x64\" />\n\
             \x20 <scd4x id=\"air\" bus=\"$i2c0\" address=\"0x62\" />\n\
             # services\n",
        ),
    },
];

#[test]
#[ignore = "spawns cargo; needs the example's target installed"]
fn examples_with_buses_compile() {
    let root = workspace_root();
    let runtime = root.join("espforge-runtime");
    // One shared target dir, so repeat runs only rebuild what changed.
    let target_dir = root.join("target").join("generated-check");

    for case in CASES {
        check_example(case, &runtime, &target_dir);
    }
}

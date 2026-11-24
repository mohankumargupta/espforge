# Espforge

A scaffolding++ tool: start with a `.espf.txt` file and a simple `src/app.rs`, and it
generates an esp32 `no_std` esp-hal based rust project with the tedious boilerplate taken
care of.

## Features
- instead of esphome style yaml, use a custom format that combines markdown headers
  with xml self-closing tags (see espf file format https://github.com/mohankumargupta/espforge/tree/main/docs/003-espf-file-format.md)
- Pre-built components and devices that sit on top of esphal https://github.com/esp-rs/esp-hal
- Supports both bare-metal blocking execution and async execution using the embassy framework.
- Wokwi integration and working examples

## Essence of espforge - blink example

Given blink.espf.txt

```xml
====================
BLINK EXAMPLE
====================

# espforge
  <project name="blink" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals

# devices
  <led id="red_led" pin="18" />

# services
```

and this src/app.rs

```rust
use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("blink example");
}

pub fn forever(ctx: &mut Context) {
    let led = device!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000);
}
```

when you run

```sh
espforge build
```

then you should have an esp32 no_std rust project ready to go.

## How `espforge` Works

```text
  YOUR PROJECT (Source of Truth)                ESPFORGE PIPELINE                GENERATED CRATE (Disposable)
  ──────────────────────────────                ─────────────────                ────────────────────────────
  project.espf / *.espf.txt  ───┐                                           ┌──► build/Cargo.toml
  src/app.rs (or app/rust/)  ───┼──► parse ─► check ─► lower ─► emit ───────┼──► build/src/generated.rs
  substitutions.env          ───┤     (espf)  (chips,   (pins,   (rust,     ├──► build/src/bin/main.rs
  drivers/*.kdl              ───┤              buses,    modes,   cargo,    ├──► build/src/app.rs
  Cargo.extra.toml / overlays───┘              traits)   ota)     wokwi)    └──► build/diagram.json & wokwi.toml
```

- **You own** your `.espf` spec, `src/app.rs`, `substitutions.env`, `drivers/`, and any overlay files (`Cargo.extra.toml`, `diagram.json`, `partitions.csv`).
- **`espforge` owns `build/`**—it is regenerated cleanly on every `espforge gen` or `espforge build` and can be deleted or git-ignored at any time.

---

## User Project Structure

A minimal project needs only two files (`.espf`/`.espf.txt` and `src/app.rs`). Optional files next to the spec customize Cargo, Wokwi, partitions, secrets, and external drivers:

```text
my-esp-app/
│
├── REQUIRED
│   ├── my-esp-app.espf          # Hardware spec (or *.espf.txt)
│   └── src/
│       └── app.rs               # setup() and forever() (or app/rust/app.rs)
│
├── OPTIONAL: SECRETS & DRIVERS
│   ├── substitutions.env        # Key=value pairs for env:NAME attributes (never copied to build/)
│   ├── drivers/                 # Local v2 driver manifests (*.kdl)
│   └── .espforge/
│       └── drivers.lock         # SHA-256 lockfile for local and cached drivers
│
├── OPTIONAL: BUILD & TOOLCHAIN OVERLAYS (layered over generated defaults)
│   ├── Cargo.extra.toml         # Merged into build/Cargo.toml (dependencies, patch, lints, profiles)
│   ├── partitions.csv           # Custom flash partition table (auto-passed to espflash)
│   ├── rust-toolchain.toml      # Custom Rust toolchain channel/targets
│   └── .cargo/
│       ├── config.toml          # Custom runner / rustflags
│       └── esp-config.toml      # Custom ESP_LOG / [env] config (or ./esp-config.toml)
│
├── OPTIONAL: WOKWI SIMULATION OVERLAYS
│   ├── diagram.json             # Custom circuit ({{ board }}, {{ serialMonitor }}, {{ <id>.pin }})
│   ├── wokwi.toml               # Custom Wokwi config
│   ├── chip.json / chip.wasm    # Custom Wokwi chip binary (+ chip/ Zig sources if enabled)
│
└── GENERATED (disposable — do not edit)
    └── build/                   # Output directory (--out <dir> to change)
        ├── Cargo.toml
        ├── build.rs
        ├── rust-toolchain.toml
        ├── diagram.json / wokwi.toml / partitions.csv
        ├── README.txt
        ├── .cargo/{config.toml, esp-config.toml}
        ├── .espforge/source.espf.txt
        └── src/{app.rs, generated.rs, lib.rs, bin/main.rs}
```

> **Path discovery rules:** When you run `espforge check`, `gen`, or `build` with no path argument, `espforge` looks for the single `.espf` or `.espf.txt` file in the current directory, and looks for app logic at `src/app.rs` first, then `app/rust/app.rs`.

---

If you want to see the examples in this repo in action, click [here](https://github.com/mohankumargupta/espforge_wokwi)

If you want to see the generated rust no_std examples look like,
click [here](https://github.com/mohankumargupta/espforge-extras/tree/main/espforge-examples-generated)

## Drivers(advanced)

Bundled drivers live in `espforge-drivers/manifests/<tag>.kdl`; a project can
add its own under `drivers/`:

```sh
espforge driver add drivers/my_sensor.kdl
espforge driver add https://example.com/my_sensor-0.1.0.tar.gz
espforge driver add git+https://example.com/my_sensor#<commit>
espforge driver add my_sensor@0.1.0 --registry https://example.com/index.json
espforge driver list
espforge driver verify
```
see https://github.com/mohankumargupta/espforge-building-blocks for working examples

## Prerequisites
**Rust**: [Install Rust](https://rustup.rs/)

## Install espforge

```shell
cargo install cargo-binstall
cargo binstall espforge
# alternatively: cargo install espforge
```

## Verify install

```shell
espforge
espforge --version
```

## Run espforge doctor

```
espforge doctor
```

Depending on the targets you want, you will either install
additional toolchain into standard rust, or if you want xtensa based
ones you will be asked to use espup install.

## Walkthrough

![espforge new](https://cdn.jsdelivr.net/gh/mohankumargupta/assets@refs/heads/main/espforge-new-clone.svg)

## Examples

`espforge examples` lists the embedded examples. `espforge new` copies one into a
directory as an editable project:

```sh
espforge examples

# asks for the category, the example, the chip and the directory:
espforge new

# or answer all three up front and it never prompts:
espforge new --example blink --chip esp32c3 --dir blink

cd blink
espforge build
```

For scripted generation, use `espforge gen --example <example> --out <dir>`. The
table below is what `espforge examples` lists:

| Category | Examples |
|---|---|
| `01.Basics` | `blink`, `blink_async`, `helloworld`, `minimal` |
| `02.Digital` | `button`, `button_async` |
| `04.Communication` | `i2c_example`, `i2c_example_async`, `i2c_multi`, `i2c_scanner`, `spi_async`, `spi_example`, `spi_example_async`, `spi_multi`, `uart_example`, `uart_example_async` |
| `05.Networking` | `http_client`, `https_client`, `ntp`, `wifi_station`, `ws_client`, `wss_client` |
| `06.Displays` | `ili9341`, `ssd1306`, `ssd1306_async`, `ssd1306_wifi` |


## License

MIT

## projects for inspiration

-  espresif iot solution https://github.com/espressif/esp-iot-solution
-  esphome
-  zephyr
-  circuitpython

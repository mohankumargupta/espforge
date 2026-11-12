# The Journey of `blink`: From `.espf` to a Generated Firmware Project

This document traces the current pipeline for the **`blink`** example
(`example.espf.txt`, `app.rs`, `diagram.json`): how it is parsed, validated,
lowered, and emitted as a standalone Rust firmware crate.

---

## 1. The Inputs

`espforge gen --example blink --out build` starts from three inputs embedded in
`espforge-examples`:

### 1. The Spec: `example.espf.txt`

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

Four sections are required. The retired `# components` block is an
unknown-section error; an LED is a device with a `pin` attribute, and the old
`gpio="$gpio18"` form is retired.

### 2. The Application Logic: `src/app.rs`

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

`device!` is the only access macro; it borrows mutably on a blocking board.

### 3. The Wokwi Overlay: `diagram.json`

```json
{
  "version": 1,
  "author": "espforge",
  "editor": "wokwi",
  "parts": [
    { "type": "{{ board }}", "id": "esp", "top": 0, "left": 0 },
    { "type": "wokwi-led", "id": "led1", "top": -40, "left": 96,
      "attrs": { "color": "red", "gpio": "{{ red_led.pin }}" } }
  ],
  "connections": [
    [ "led1:A", "esp:{{ red_led.pin }}", "green", [] ],
    [ "led1:C", "esp:GND.10", "black", [] ]
  ]
}
```

---

## 2. Pipeline Overview

```
[CLI] espforge
  │
  ├──► [1. Source resolution] espforge-core::source & espforge-examples
  │      Locates the embedded example assets.
  │
  ├──► [2. Validation and lowering] espforge-core::pipeline
  │      ├──► espf::parser              Four sections, nodes, spans.
  │      ├──► espforge-elements::core    <project>, <chip>, <runtime>.
  │      ├──► espforge-elements::options Build options (alloc, wokwi).
  │      └──► espforge-elements::lower   Claims GPIO18, lowers <led>.
  │
  ├──► [3. Code generation] espforge-emit
  │      ├──► codegen::rust             generated.rs, main.rs, lib.rs, build.rs.
  │      ├──► codegen::cargo            Cargo.toml with toml_edit.
  │      ├──► codegen::config           .cargo/config.toml, toolchain.
  │      ├──► codegen::overlay          {{ board }} and {{ red_led.pin }}.
  │      └──► manifest                  FNV-1a checksums for drift tracking.
  │
  └──► [4. Writing and compilation] espforge-emit::write_all & cargo build
         Writes build/, copies app.rs, runs cargo build.
```

---

## 3. Walkthrough

### Phase 1: Source resolution

- `espforge/src/commands/generate.rs` parses `GenArgs` and calls
  `espforge_core::source::resolve_source`.
- `resolve_source` asks `espforge_examples::resolve_name("blink")`, which finds
  `01.Basics/blink` in the embedded tree.
- The returned `Source` carries the spec text and references to the example's
  `app/rust/app.rs` and `diagram.json`.

### Phase 2: Parsing, validation, lowering

`espforge-core/src/pipeline.rs` calls `validate(&source.text, overrides)`.

1. **Parser** (`espf/src/parser.rs`): `parse_espf` strips banner comments, reads
   section headers, requires `# espforge`, `# peripherals`, `# devices`,
   `# services`, and parses each self-closing node with its span. `# components`
   is rejected as an unknown section.
2. **Core settings** (`espforge-elements/src/core.rs`): `<project name="blink">`,
   `<chip type="esp32c3">` (checked against `esp-metadata-generated`) and
   `<runtime type="blocking">` become `Meta`; `console` defaults to UART.
3. **Options** (`espforge-elements/src/options.rs`): `defaults` enables
   `unstable-hal`, `alloc` and `wokwi`; `embassy` and `wifi` are derived, and
   both are false here.
4. **Lowering** (`espforge-elements/src/lower.rs`): `LowerCx` claims the pins and
   builds the board IR.
   - `# peripherals` is empty.
   - `<led id="red_led" pin="18" />` is a device: the `led` recipe declares an
     output pin, so lowering claims GPIO18 through the chip's pin table and
     rejects an input-only pin or a conflicting claim.
   - The result is a `Device` with `feature = "led"`,
     `ty = "espforge_runtime::Led"` and the pin's expression as an argument.
   - `# services` is empty.

### Phase 3: Emission

`espforge-core/src/project.rs` calls `espforge_emit::generate_from_board`, which
builds each artifact in `codegen/`.

#### `src/generated.rs`

`collect(board)` asks each device to emit itself, then assembles the structs:

```rust
pub struct Devices {
    pub red_led: espforge_runtime::Led,
}

impl Devices {
    pub fn new(
        registry: &mut PeripheralRegistry,
    ) -> Result<Self, espforge_runtime::DeviceError> {
        Ok(Self {
            red_led: espforge_runtime::Led::new(
                unsafe { registry.peripherals.GPIO18.clone_unchecked() },
                false,
            )
            .map_err(|kind| espforge_runtime::DeviceError {
                device: "red_led",
                kind,
            })?,
        })
    }
}

pub struct Services {}
```

`Devices::new` is fallible: a part that does not answer becomes a named
`DeviceError` that the boot site logs and halts on. On an embassy board the
fields would be `core::cell::RefCell<…>` so a shared `Context` can hand out a
guard; a blocking board keeps the bare field and `&mut`.

#### `src/lib.rs`

```rust
#[macro_export]
macro_rules! device {
    ($ctx:expr, $name:ident) => {
        &mut $ctx.devices.$name
    };
}

#[macro_export]
macro_rules! component {
    ($ctx:expr, $name:ident) => {
        $crate::device!($ctx, $name)
    };
}

pub struct Context {
    pub logger: espforge_runtime::Logger,
    pub delay: espforge_runtime::Delay,
    pub devices: Devices,
    pub services: Services,
}
```

#### `src/bin/main.rs`

```rust
#[esp_hal::main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();
    esp_println::print!("\x1b[20h");
    esp_bootloader_esp_idf::esp_app_desc!();
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut registry = PeripheralRegistry::new(peripherals);

    let logger = espforge_runtime::Logger::new();
    let delay = espforge_runtime::Delay::new();

    let devices = match Devices::new(&mut registry) {
        Ok(devices) => devices,
        Err(error) => {
            logger.error(format_args!("{}", error));
            loop {
                esp_hal::delay::Delay::new().delay_millis(1_000);
            }
        }
    };
    let services = Services {};

    let ctx = Context { logger, delay, devices, services };
    let ctx = CTX.init(ctx);
    app::setup(ctx);
    loop {
        app::forever(ctx);
    }
}
```

#### Configuration files

- `Cargo.toml` is **merged**: espforge owns the managed keys (dependencies,
  profiles) and leaves a user's other keys alone. An edited managed key is
  restored with a warning naming it.
- `.cargo/config.toml` sets the runner (`espflash flash --monitor --chip
  esp32c3`), the target (`riscv32imc-unknown-none-elf`) and the frame-pointer
  flag.
- `rust-toolchain.toml` pins the channel, target and `rust-src`.
- `wokwi.toml` points at the built ELF.

#### Overlay

`codegen/overlay.rs` replaces `{{ board }}` with
`board-esp32-c3-devkitm-1` and `{{ red_led.pin }}` with the pin the lowered
device recorded, so the diagram has `"esp:18"` rather than a template tag.

### Phase 4: Writing to disk

`writer.rs`/`project.rs` produce:

```
build/
├── .cargo/{config.toml, esp-config.toml}
├── .espforge/{manifest.json, source.espf.txt}
├── src/{app.rs, generated.rs, lib.rs, bin/main.rs}
├── build.rs
├── Cargo.toml
├── diagram.json
├── README.txt
├── rust-toolchain.toml
└── wokwi.toml
```

`app.rs` is copied verbatim; the spec is preserved for auditability.

### Phase 5: Compilation

`espforge build` runs `cargo build` in `build/`. Cargo compiles
`src/bin/main.rs` for `riscv32imc-unknown-none-elf`, `esp_hal::init` starts the
clocks, `Devices::new` builds the LED, `Context` is handed to `app::setup`, and
`app::forever` toggles it every second.

---

## 4. Input to Output Map

| Input line | Processing | Generated output |
|---|---|---|
| `<project name="blink" />` | `espf` parser, `elements::core`, `emit::cargo` | `[package] name = "blink"`; `use blink::{…}` |
| `<chip type="esp32c3" />` | `elements::chips`, `emit::config`, `emit::wokwi` | target `riscv32imc-unknown-none-elf`; board id |
| `<runtime type="blocking" />` | `elements::core`, `emit::rust` | `#[esp_hal::main]` entry point |
| `<led id="red_led" pin="18" />` | `elements::lower` (device recipe, pin table) | claims GPIO18; `pub red_led: espforge_runtime::Led`; `DeviceError` mapping |
| `device!(ctx, red_led)` | `emit::rust` (`lib.rs`) | `&mut ctx.devices.red_led` (blocking) or `borrow_mut()` (embassy) |
| `"esp:{{ red_led.pin }}"` | `emit::overlay` | `"esp:18"` in `diagram.json` |

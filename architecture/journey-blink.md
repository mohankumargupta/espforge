# The Journey of `blink`: From `.espf` to a Generated Firmware Project

This document traces the complete end-to-end pipeline of how the **`blink`** example (`example.espf.txt`, `app.rs`, and `diagram.json`) is processed, validated, lowered, and emitted as a standalone Rust firmware crate.

---

## 1. The Inputs

When a user runs `espforge gen --example blink --out build` (or `espforge build`), the system starts with three distinct inputs embedded in `espforge-examples`:

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
  <gpio id="gpio18" pin="18" direction="output" />

# components
  <led id="red_led" gpio="$gpio18" />

# devices

# services
```

### 2. The Application Logic: `src/app.rs`
```rust
use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("blink example");
}

pub fn forever(ctx: &mut Context) {
    let led = component!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000);
}
```

### 3. The Wokwi Overlay: `diagram.json`
```json
{
  "version": 1,
  "author": "espforge",
  "editor": "wokwi",
  "parts": [
    { "type": "{{ board }}", "id": "esp", "top": 0, "left": 0 },
    { "type": "wokwi-led", "id": "led1", "top": -40, "left": 96, "attrs": { "color": "red", "gpio": "{{ red_led.pin }}" } }
  ],
  "connections": [
    [ "led1:A", "esp:{{ red_led.pin }}", "green", [] ],
    [ "led1:C", "esp:GND.10", "black", [] ]
  ]
}
```

---

## 2. Architectural Pipeline & Crate Traversal

Execution flows through five discrete crates in strict sequence:

```
[CLI] espforge
  │
  ├──► [1. Source Resolution] espforge-core::source & espforge-examples
  │      Locates embedded example assets (text, app.rs, diagram.json).
  │
  ├──► [2. Front-Half Validation] espforge-core::pipeline
  │      ├──► espf::parser             Lexing, section splitting, node syntax.
  │      ├──► espforge-elements::core   Validates <project>, <chip>, <runtime>.
  │      ├──► espforge-elements::options Resolves build options (alloc, wokwi).
  │      └──► espforge-elements::lower  Claims GPIO18 & lowers <led>.
  │
  ├──► [3. Back-Half Code Generation] espforge-emit
  │      ├──► codegen::rust            Emits generated.rs, main.rs, lib.rs, build.rs.
  │      ├──► codegen::cargo           Constructs Cargo.toml with toml_edit.
  │      ├──► codegen::config          Constructs .cargo/config.toml, toolchain.
  │      ├──► codegen::overlay         Substitutes {{ board }} and {{ red_led.pin }}.
  │      └──► manifest                 Computes FNV-1a checksums for drift tracking.
  │
  └──► [4. Project Writing & Compilation] espforge-emit::write_all & cargo build
         Writes build/ directory, copies app.rs, and runs cargo build.
```

---

## 3. Detailed Walkthrough by Phase

---

### Phase 1: CLI Entry & Source Resolution

#### 1. CLI Dispatch
- **File:** `espforge/src/main.rs` & `espforge/src/commands/generate.rs`
- The user runs `espforge gen --example blink --out build`.
- Clap parses arguments into `GenArgs { example: Some("blink"), out: "build", ... }`.
- `generate::execute(&args)` delegates to `espforge_core::source::resolve_source`.

#### 2. Embedded Example Lookup
- **File:** `espforge-core/src/source.rs` & `espforge-examples/src/lib.rs`
- `resolve_source(None, Some("blink"), cwd)` calls `espforge_examples::resolve_name("blink")`.
- `resolve_in` searches the compile-time embedded directory `EXAMPLES_DIR` and resolves the short name `"blink"` to `"01.Basics/blink"`.
- It returns an `espforge_core::source::Source`:
  - `name`: `"embedded:01.Basics/blink"`
  - `text`: Contents of `example.espf.txt`
  - `example`: Reference to the embedded `Example` containing `app/rust/app.rs` and `diagram.json`.

---

### Phase 2: Syntax Parsing & Validation Pipeline

The pipeline is triggered in `espforge-core/src/pipeline.rs` via `validate(&source.text, overrides)`.

```
                  ┌───────────────────────────────┐
                  │      validate(src, flags)     │
                  └───────────────┬───────────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         ▼                        ▼                        ▼
  [espf::parser]         [elements::core]       [elements::options]
  Sections & Nodes       Meta Settings           ResolvedOptions
         │                        │                        │
         └────────────────────────┼────────────────────────┘
                                  │
                                  ▼
                        [elements::lower]
                    LowerCx / ResourceGraph
                 Claims Pin 18 ──► Board IR
```

#### Step 2.1: Grammar & AST Assembly (`espf`)
- **File:** `espf/src/parser.rs`
- `parse_espf(src)` runs `winnow` parsers:
  1. `skip_trivia`: Strips comments starting with `=` (`==================== BLINK EXAMPLE ====================`).
  2. `section_header`: Detects `# espforge`, `# peripherals`, `# components`, `# devices`, and `# services`.
  3. `REQUIRED_SECTIONS`: Enforces that all 5 mandatory sections exist.
  4. `parse_node`: Parses self-closing XML-like elements into `ParsedNode`:
     - `<project name="blink" />`
     - `<chip type="esp32c3" />`
     - `<runtime type="blocking" />`
     - `<gpio id="gpio18" pin="18" direction="output" />`
     - `<led id="red_led" gpio="$gpio18" />`

#### Step 2.2: `# espforge` Core Settings
- **File:** `espforge-elements/src/core.rs`
- `core_from_section(offset, &sections["espforge"])` extracts:
  - `name`: `"blink"`
  - `chip`: `"esp32c3"` (verified against `espforge-elements/src/chips.rs` using `esp-metadata-generated`)
  - `runtime`: `Runtime::Blocking`
  - `console`: Defaults to `Console::Uart`
- Produces `Meta { name: "blink", chip: "esp32c3", runtime: Blocking, console: Uart, ... }`.

#### Step 2.3: Build Options Resolution
- **File:** `espforge-elements/src/options.rs`
- Compares user answers with the options schema in `espforge-schema/src/espf.schema.kdl`:
  - `defaults="true"` (default): Enables `unstable-hal`, `alloc`, and `wokwi`.
  - `embassy`: Derived flag; resolves to `false` because runtime is `blocking`.
  - `wifi`: Derived flag; resolves to `false` because there is no `<wifi>` node in `# peripherals`.
  - Produces `ResolvedOptions`.

#### Step 2.4: Hardware Lowering & Pin Claiming
- **File:** `espforge-elements/src/lower.rs`
- Calls `lower(meta, peripherals, components, devices, services)` using `LowerCx`:

1. **Lower `# peripherals`:**
   - Evaluates `<gpio id="gpio18" pin="18" direction="output" />`.
   - Dispatches to `espforge-elements/src/peripherals/gpio.rs`.
   - Calls generated attribute parser `crate::generated::gpio::Attrs::from_node(node)`.
   - Adds to context: `cx.add_gpio(GpioDecl { id: "gpio18", pin: 18, direction: Some(Output), ... })`.
   - *Note:* The `<gpio>` node does **not** claim the pin yet; it records the physical capability.

2. **Check Bus Declarations:**
   - `check_bus_gpio_declarations(&mut cx)` verifies no bus controller pins clash with declared GPIOs. (Passes; no buses).

3. **Lower `# components`:**
   - Evaluates `<led id="red_led" gpio="$gpio18" />`.
   - Looks up `led` in `espforge-elements/src/registry.rs`.
   - `led` is registered in `registry::recipes()` as a **`RecipeElement`** (`espforge-elements/src/components/recipe.rs`).
   - The recipe reads its definition from `espf.schema.kdl`:
     ```kdl
     recipe pin="gpio" direction="output" runtime="espforge_runtime::Led"
     ```
   - Resolves reference: looks up `gpio18` via `cx.gpio("gpio18")`.
   - Verifies direction: `gpio18` is `Output`, matching the recipe's requirement.
   - **Claims Pin 18:** Calls `cx.claim_pin(Pin(18), Owner::new(Recipe("led"), "red_led"), span, PinUse::Output)`.
     - Recorded in `ResourceGraph`. Any duplicate or conflicting claim will trigger an error.
   - Pushes to component list:
     ```rust
     Component::Recipe(RecipeComponent {
         id: Ident("red_led"),
         tag: "led",
         pin: ClaimedPin(18),
         runtime: "espforge_runtime::Led",
         flags: vec![false], // active_low defaults to false
     })
     ```

4. **Lower `# devices` and `# services`:**
   - Both sections are empty; lowering returns `Ok(Board)`.

---

### Phase 3: Code Generation & Artifact Emission

- **File:** `espforge-core/src/project.rs` calls `write_checked(...)`.
- `espforge-emit::generate_from_board` initiates emission.

#### Step 3.1: Rust Code Generation
- **File:** `espforge-emit/src/codegen/rust.rs`
- Calls `collect(board)`, iterating over `board.components`:
  - `RecipeComponent::emit` executes:
    - **Field:** `pub red_led: espforge_runtime::Led,`
    - **Init Expression:**
      ```rust
      red_led: espforge_runtime::Led::new(
          unsafe { registry.peripherals.GPIO18.clone_unchecked() },
          false,
      ),
      ```
    - **Safety Comment Anchor:** Records anchor `"red_led: espforge_runtime::Led::new("` with explanation `"SAFETY: espforge checked that no other component uses GPIO18."`.

- Generates 4 Rust files:

1. **`src/generated.rs`:**
   - Contains `PeripheralRegistry`, `Components`, empty `Devices`, and empty `Services`.
   - Token stream is formatted with `prettyplease`.
   - `espforge-emit/src/codegen/comments.rs::apply` searches for the safety anchor and injects the `// SAFETY:` comment directly above the constructor.
   ```rust
   // GENERATED BY ESPFORGE — DO NOT EDIT. Source of truth is the project .espf file.
   #![allow(dead_code, unused_variables)]
   ...
   pub struct Components {
       pub red_led: espforge_runtime::Led,
   }

   impl Components {
       pub fn new(registry: &mut PeripheralRegistry) -> Self {
           Self {
               // SAFETY: espforge checked that no other component uses GPIO18.
               red_led: espforge_runtime::Led::new(
                   unsafe { registry.peripherals.GPIO18.clone_unchecked() },
                   false,
               ),
           }
       }
   }
   pub struct Devices {}
   pub struct Services {}
   ```

2. **`src/lib.rs`:**
   - Emits `#![no_std]`, module declarations (`pub mod app;`, `pub mod generated;`), and the `component!`, `device!`, and `service!` helper macros:
   ```rust
   #[macro_export]
   macro_rules! component {
       ($ctx:expr, $name:ident) => {
           &$ctx.components.$name
       };
   }
   ```
   - Defines `Context` containing `logger`, `delay`, `components`, `devices`, and `services`.

3. **`src/bin/main.rs`:**
   - Emits the bare-metal entry point for `esp-hal`:
   ```rust
   #![no_std]
   #![no_main]

   use blink::{app, Components, Context, Devices, PeripheralRegistry, Services, CTX};
   use esp_backtrace as _;

   #[esp_hal::main]
   fn main() -> ! {
       esp_println::logger::init_logger_from_env();
       esp_println::print!("\x1b[20h");
       esp_bootloader_esp_idf::esp_app_desc!();
       let peripherals = esp_hal::init(esp_hal::Config::default());
       #[allow(unused_mut)]
       let mut registry = PeripheralRegistry::new(peripherals);

       let logger = espforge_runtime::Logger::new();
       let delay = espforge_runtime::Delay::new();

       let components = Components::new(&mut registry);
       let devices = Devices {};
       let services = Services {};

       let ctx = Context { logger, delay, components, devices, services };
       let ctx = CTX.init(ctx);
       app::setup(ctx);
       loop {
           app::forever(ctx);
       }
   }
   ```

4. **`build.rs`:**
   - Emits linker script setup (`-Tlinkall.x`) and diagnostic handler hooks.

---

#### Step 3.2: Configuration & Manifest Generation
- **Files:** `espforge-emit/src/codegen/plan.rs`, `cargo.rs`, `config.rs`, `wokwi.rs`
- The `FilePlan` dictates all non-Rust files to emit:

1. **`Cargo.toml` (`codegen/cargo.rs`):**
   - Uses `toml_edit` to generate:
     - `[package]` name = `"blink"`, edition = `"2024"`, rust-version = `"1.95"`.
     - `[dependencies]`:
       - `espforge-runtime = { version = "0.7.0" }`
       - `esp-hal = { version = "~1.2.2", features = ["esp32c3", "unstable", "log-04"] }`
       - `esp-println`, `esp-backtrace`, `esp-alloc`, `log`, `static_cell`.
     - `[profile.dev]` (`opt-level = "s"`) and `[profile.release]` (`lto = "fat"`).

2. **`.cargo/config.toml` (`codegen/config.rs`):**
   - Configures runner: `espflash flash --monitor --chip esp32c3`.
   - Sets target: `riscv32imc-unknown-none-elf`.
   - Sets rustflags: `-C force-frame-pointers`.

3. **`rust-toolchain.toml`:**
   - `channel = "stable"`, `targets = ["riscv32imc-unknown-none-elf"]`, `components = ["rust-src"]`.

4. **`wokwi.toml` (`codegen/wokwi.rs`):**
   - Points to `target/riscv32imc-unknown-none-elf/debug/blink`.

---

#### Step 3.3: Overlay & Placeholder Resolution
- **Files:** `espforge-emit/src/project.rs` & `codegen/overlay.rs`
- `layer_overlay` checks for example overlay files. It identifies `diagram.json`.
- `codegen::overlay::render` processes the template tags:
  1. `{{ board }}`: Looks up chip `"esp32c3"` in `codegen/wokwi.rs` -> replaces with `"board-esp32-c3-devkitm-1"`.
  2. `{{ red_led.pin }}`: Looks up component `"red_led"` in `board.components`.
     - Calls `RecipeComponent::overlay_attr("pin")` -> returns `"18"`.
     - Replaces all occurrences of `{{ red_led.pin }}` with `"18"`.
- The final `diagram.json` has concrete board IDs and pin connections:
  ```json
  [ "led1:A", "esp:18", "green", [] ]
  ```

---

#### Step 3.4: Drift Manifest & Readme Generation
- **Files:** `espforge-emit/src/manifest.rs` & `project.rs`
- `emit_manifest`: Calculates 64-bit FNV-1a hashes of all generated `Owned` files (`Cargo.toml`, `src/generated.rs`, `diagram.json`, etc.) and stores them in `.espforge/manifest.json`.
- `readme`: Produces `README.txt` documenting machine-owned vs. seed-once files.

---

### Phase 4: Writing to Disk

- **Files:** `espforge-emit/src/lib.rs` (`write_all`) & `espforge-core/src/project.rs`
- Writes files to the output directory (e.g. `build/`):

```
build/
├── .cargo/
│   ├── config.toml           # SeedOnce: runner & target config
│   └── esp-config.toml       # SeedOnce: ESP_LOG="info"
├── .espforge/
│   ├── manifest.json         # Owned: SHA/checksum tracking
│   └── source.espf.txt       # Verbatim copy of input example.espf.txt
├── src/
│   ├── bin/
│   │   └── main.rs           # Owned: system boot & loop runner
│   ├── app.rs                # User Logic: copied verbatim from app/rust/app.rs
│   ├── generated.rs          # Owned: Components struct wiring GPIO18
│   └── lib.rs                # Owned: Context, macros (component!), re-exports
├── build.rs                  # Owned: linker script helper
├── Cargo.toml                # Owned: dependencies & profiles
├── diagram.json              # Owned: Wokwi schematic with GPIO18 wired
├── README.txt                # Owned: generated project documentation
├── rust-toolchain.toml       # SeedOnce: toolchain target pins
└── wokwi.toml                # Owned: Wokwi emulator configuration
```

- **App Logic Injection:** `project.rs` copies `app/rust/app.rs` verbatim to `build/src/app.rs`.
- **Source Preservation:** `project.rs` saves the source spec to `build/.espforge/source.espf.txt` for auditability.

---

### Phase 5: Compilation (`espforge build`)

When invoked via `espforge build`, execution continues in `espforge/src/commands/build.rs`:
1. Spawns `cargo build` inside the generated directory `build/`.
2. Cargo reads `.cargo/config.toml` and compiles `src/bin/main.rs` targeting `riscv32imc-unknown-none-elf`.
3. In `main.rs`:
   - `esp_hal::init(...)` initializes the ESP32-C3 clocks.
   - `Components::new(&mut registry)` configures GPIO18 as a push-pull output.
   - `Context` is instantiated and passed to `app::setup(ctx)`.
   - `loop { app::forever(ctx); }` begins toggling `ctx.components.red_led` every 1000 ms.

---

## 4. Mapping: Input Spec to Output Code

| Input Line in `blink` | Processing Crates / Files | Generated Output |
|---|---|---|
| `<project name="blink" />` | `espf/src/parser.rs`<br>`elements/src/core.rs`<br>`emit/src/codegen/cargo.rs` | `[package]`<br>`name = "blink"` in `Cargo.toml`;<br>`use blink::{...}` in `main.rs` |
| `<chip type="esp32c3" />` | `elements/src/chips.rs`<br>`emit/src/codegen/config.rs`<br>`emit/src/codegen/wokwi.rs` | Target `riscv32imc-unknown-none-elf`;<br>`board-esp32-c3-devkitm-1` in `diagram.json` |
| `<runtime type="blocking" />` | `elements/src/core.rs`<br>`emit/src/codegen/rust.rs` | `#[esp_hal::main]` synchronous entry point in `main.rs` |
| `<gpio id="gpio18" pin="18" direction="output" />` | `elements/src/peripherals/gpio.rs` | Validates pin capability; registers `gpio18` as valid output pin |
| `<led id="red_led" gpio="$gpio18" />` | `elements/src/components/recipe.rs`<br>`emit/src/codegen/rust.rs` | Claims Pin 18;<br>Emits `pub red_led: espforge_runtime::Led`<br>in `src/generated.rs` |
| `component!(ctx, red_led)` | `emit/src/codegen/rust.rs` (lib.rs) | Expands to `&ctx.components.red_led` |
| `"esp:{{ red_led.pin }}"` in `diagram.json` | `emit/src/codegen/overlay.rs` | Resolved to `"esp:18"` in `diagram.json` |

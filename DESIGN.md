# espforge — Redesign Sketch (Option B + Option A)

Scratch design doc. NOT yet implemented. Goal: replace the 13-crate, by-*kind*
workspace with a small, testable compiler pipeline plus a single-declaration
component model.

## Why (current pain, with evidence)

- **Same concept declared 3–4× by hand.** A component's config lives in
  `espforge_common`, its plugin impl in `espforge_components_builder`, its runtime
  in `espforge_components`, re-exported elsewhere. Adding a driver touches ≥5 files
  across ≥4 crates.
- **Untyped config.** `Component`/`Device` are `driver: String` +
  `properties: serde_yaml_ng::Value` (`espforge_configuration/src/components/mod.rs`).
  Each plugin re-parses the same `Value` (`button.rs:13,18,24`) and hand-strips a
  `"$"` pin prefix.
- **Fail-late validation.** `ComponentProvisioner` checks components at parse time
  (`parse/components.rs:36`) but `DeviceProvisioner` checks nothing
  (`parse/devices.rs:19-27`); `wifi` validation is commented out (`esp32.rs:94-99`).
  Bad devices only explode at codegen.
- **Fragile discovery.** Plugins are registered via link-time `inventory` plus a
  `std::hint::black_box` `init()` hack (`espforge_components_builder/src/lib.rs:10`)
  called from `main.rs`. Two registration paths coexist (`inventory::submit!` in
  `esp32.rs:105` vs manual `ConfigParserBuilder` in `parse/mod.rs:68-74`).
- **Hardcoded drift.** `espforge_codegen/src/context.rs:85-99` derives network
  features from a string list instead of `plugin.required_features()`.
- **Untestable monolith.** `ProjectCompiler::run` (`compile/mod.rs:35`) mixes I/O,
  `println!`, and orchestration; no unit tests anywhere in parse/codegen/config.
- **Dead code.** `config_struct!` macro (`config.rs:8`) is unused.

## Target shape

A compiler-like pipeline where each stage is a pure `fn(In) -> Result<Out>` over
explicit types. The **DeviceTree IR** is the single inspectable artifact that every
output format is generated from.

```
 YAML ─load→ RawConfig ─parse→ SemanticModel ─validate→ SemanticModel
                                                      │
                                                lower→ DeviceTree (IR)
                                                      │
                       emit ──→ RustFirmware │ CargoToml │ Wokwi │ JsonDump │ DotDiagram
                                                      │
                                                write→ artifacts on disk
```

## Stage signatures

```rust
// espforge-core/src/pipeline.rs
pub fn load(path: &Path) -> Result<RawConfig>;                       // IO only
pub fn parse(raw: RawConfig) -> Result<SemanticModel>;               // text -> typed model
pub fn validate(model: &SemanticModel) -> Result<Vec<Diag>>;        // errors as data, never panic
pub fn lower(model: SemanticModel) -> Result<DeviceTree>;           // model -> IR
pub fn emit(tree: &DeviceTree, fmt: OutputFormat) -> Result<Artifact>; // IR -> any target
pub fn write(dir: &Path, art: Artifact) -> Result<()>;              // IO only

pub enum OutputFormat { RustFirmware, CargoToml, Wokwi, JsonDump, DotDiagram }
pub enum Artifact { Sources(Vec<File>), Json(serde_json::Value), Graph(String) }
```

- No `Value` pass-through: `parse` deserializes straight into `SemanticModel`.
- Validator returns `Vec<Diag>` (span-aware structured errors), not panics. All
  `println!` lives only in the CLI layer via `log`.

## SemanticModel (validated config)

```rust
pub struct SemanticModel {
    pub project: ProjectInfo,
    pub platform: Platform,                 // chip + esp32 peripherals, enum-validated
    pub components: Vec<ComponentInstance>,
    pub devices: Vec<DeviceInstance>,
}

pub struct ComponentInstance {
    pub name: Ident,                        // instance name
    pub kind: ComponentKind,                // resolved via registry lookup
    pub pins: HashMap<PinRole, PinRef>,     // typed, not "$gpio" strings
    pub props: ValidatedProps,              // already deserialized + checked
}

pub struct PinRef { pub bank: GpioBank, pub num: u8 }  // real type, replaces "$pin" strings
```

## DeviceTree (the IR everything emits from)

```rust
pub struct DeviceTree {
    pub root: Node,
    pub pins: PinAllocator,                 // single owner of GPIO
}

pub enum Node { Peripheral(PeripheralNode), Component(ComponentNode), Device(DeviceNode) }

pub struct ComponentNode {
    pub name: Ident,
    pub ty: TypePath,                       // e.g. espforge_components::led::LED
    pub init: InitExpr,                     // token stream bound to verified pins
    pub deps: Vec<Dep>,
}
```

Because `PinAllocator` is the sole owner of pins, generated init references
`pins.led_gpio` — a type-checked binding — instead of
`registry.led_gpio.borrow_mut().take().unwrap()` (current `led.rs:37` footgun).
A typo is a compile error in `emit`, surfaced as a `Diag`, never an on-device panic.

## Folding in Option A (single-declaration components)

Once `DeviceTree` is stable, a component is declared **once**:

```rust
#[espforge::component(kind = "led", features = "led")]
pub struct Led {
    gpio: PinRef,                           // becomes a PinRole in the IR automatically
    active_low: bool = false,
}
// derive generates: SemanticModel schema + validate() entry + DeviceTree node emitter
```

`ComponentKind`, `features`, and `generate_code` all live in one place. The registry
becomes **derived data** over this list — no hand-written `inventory` submission, no
separate `espforge_components_builder` crate, no `black_box` init list.

## Resulting crate layout

```
espforge-core      model + IR + stages + validation        (pure, unit-testable)
espforge-macros    #[component] derive
espforge-cli       IO, clap, log output
```

13 crates → 3. The only "magic" is the single derive.

## Migration path (incremental, non-breaking)

1. Add `espforge-core` with `SemanticModel` + `DeviceTree` + stage stubs. No macros yet.
2. Port `parse` → `parse()` + `validate()` for 2 components (LED, Button). Add unit
   tests around both — currently none exist.
3. Port `compile` emit → `emit(DeviceTree, RustFirmware)`. Keep old pipeline running
   in parallel; diff generated output.
4. Introduce `#[espforge::component]` derive; migrate LED/Button through it; delete
   their hand-written builder files + `inventory`/black_box registration.
5. Once stable, delete `espforge_components_builder`, `espforge_devices_builder`,
   `espforge_configuration` glue, and retire `config_struct!`.
```

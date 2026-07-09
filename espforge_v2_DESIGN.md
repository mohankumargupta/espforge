# espforge v2 — Design (compiled from ADR log)

This document is the compiled output of an 11-decision first-principles redesign
session (`/grill-with-docs`). It is ground-up: it does not inherit the v1
13-crate layout. The authoritative record of *why* is the ADR log under `adr/`.

Status: design-only + active `create`/`build` work on the `espforgev2` branch.
The pipeline, model, emitters, and examples crate exist; the v2 `esp32`
named-map shape and the `create` subcommand are the current work.

## 1. Problem statement
A user who knows electronics/ESP32 but not HAL boilerplate wants to describe a
board **declaratively** and get a correct, `no_std` Rust firmware project without
hand-writing init/ownership wiring. espforge is a **YAML/`app.rs`-driven generator
+ maintainer**: it emits the full project from a small set of source-of-truth
inputs and **keeps regenerating idempotently** as the spec evolves. The generated
project is machine-owned output; the user only authors behavior.

## 2. Domain model
A **Project** is the whole spec: metadata + **Peripherals** + **Components** +
**Devices** + app logic. These form a typed DAG:
- **Peripheral** → raw ESP32 hardware (pin, I2C/SPI/UART bus, WiFi).
- **Component** → a reusable named **capability with an API**; hardware-backed
  (I2cDevice, LED) *or* software-service (http, mqtt, websockets, voice_control).
- **Device** → a **terminal** high-level driver consuming components (± pins) to
  deliver an end function (ssd1306, ili9341). Consumed only by the app.

Wiring is a DAG: components may be consumed by components and devices; **devices
are terminal** (no device-on-device). Bus-sharing lives at the component tier
(one `I2cDevice` shared by reference by many devices).

## 3. Ubiquitous language
| Term | Meaning |
|---|---|
| Peripheral | raw ESP32 hardware resource |
| Component | reusable named capability (HW or SW service) with an API |
| Device | terminal high-level driver consuming components ± pins |
| Instance | one named occurrence of a component/device in a project |
| ResourceRef / PinRef | typed reference value object to a named resource |
| Project | the whole spec (metadata + peripherals + components + devices + app) |
| IR / DeviceTree | validated intermediate representation all emitters read |
| Binding | a driver's contract → codegen glue (in `espforge-bindings`) |
| Driver | a capability's `no_std` runtime implementation (in `espforge-runtime`, with distinct `components` and `devices` modules) |

## 4. Core entities / value objects
- **Source-of-truth inputs:** project YAML, `app.rs`, `dependencies.toml`,
  optional `.cargo/config.toml` override, optional `diagram.json` override.
- **`DeviceTree` IR:** `project_meta`, typed `peripherals`, typed `instances`
  (resolved `with:` params + dependency edges w/ kinds + access paths),
  `init_order`, `required_features`, flags (`is_embassy`, `has_alloc`,
  `has_wifi`, `needs_delay`, `needs_stack`).
- **Value objects:** `PinRef`, `ResourceRef`, `Diag` (file:line:col + field path
  + fix hint), `Artifact` (path + content), `Driver` trait (name, kind, required
  features, deps, `generate`).

## 5. Architectural principles
1. **YAML/app.rs is source of truth; everything else is generated.** Generation
   is idempotent and drift-detecting (enforcement-grade manifest).
2. **Single Rust backend; IR + multi-emitter.** Multi-language backend is a
   non-goal for v1.
3. **Typed, not stringly-typed.** Instances carry kinds; references are typed VOs;
   validation is structural, not `Value`-re-parsing.
4. **Pure pipeline over monolith.** Every stage is `fn(In) -> Result<Out>`;
   emitters read only the IR.
5. **Make invalid states unrepresentable.** Move-by-value ownership; double-claims
   caught at YAML time (D9) and statically at compile time (D8).
6. **Explicit over implicit**: addresses, active-low, frequencies stay in YAML.
7. **Host/target wall is crate-level.** `no_std` runtime is a leaf; host never
   links it.

## 6. Workspace layout (5 crates)
| Crate | Role | Boundary |
|---|---|---|
| `espforge` | CLI binary + parse/resolve/emit orchestration | host / std |
| `espforge-model` | `DeviceTree` IR, `Driver` trait, registry **types** | host / std; depends on neither host nor target |
| `espforge-bindings` | in-tree `generate` impls + driver registry list | host / std |
| `espforge-runtime` | `no_std` runtime impls, split into distinct `components` and `devices` modules | target / no_std (leaf) |
| `espforge-examples` | sample projects | CI integration gate |

## 7. Module boundaries & 8. dependency rules
- `espforge-runtime` depends **only** on `esp-hal`/`embedded-hal` (leaf).
- `espforge-model` depends on neither host nor target.
- `espforge` + `espforge-bindings` are host/std and reference `espforge-runtime`
  **only by name/path inside emitted token streams** — never link it into the
  host build.
- **No cross-boundary cycles.**
- `espforge-runtime` keeps **distinct `components` and `devices` modules** — the
  runtime mirrors the three-tier domain spine (ADR-003) and deliberately preserves
  the component/device distinction that esphome blurs.
- Per driver: 2 files — generate-impl in `espforge-bindings`, runtime-impl in
  `espforge-runtime` (under its `components` or `devices` module as appropriate).

## 8b. Crate publishing & local override

All three non-example crates are **published on crates.io**: `espforge-model`,
`espforge-bindings`, and `espforge-runtime` (the `espforge` CLI is published as a
binary). This mirrors v1, which published its platform/component crates.

The `ESPFORGE_USE_LOCAL` environment variable (set to a local espforge checkout)
flips espforge crates to **local path dependencies** instead of the published
versions. It applies at two levels:

- **Generated project** `Cargo.toml` — `espforge build` reads `ESPFORGE_USE_LOCAL`
  and emits `espforge-runtime = { path = "<local>/espforge-runtime" }` (and any
  other `espforge-*` crate the project references) instead of `espforge-runtime =
  "0.1"`. Resolution is general over all `espforge-*` crates (see `espforge_dep`
  in `emit/rust.rs`).
- **The tool itself** — when `espforge` is installed/published and `ESPFORGE_USE_LOCAL`
  is set, its own `espforge-bindings` / `espforge-model` deps resolve to the local
  checkout. Within the v2 workspace these are already path deps, so the override
  only matters for an installed/published `espforge`.

## 9. Extension / plugin model
One module/file per driver via a derive macro (typed config + `using` name +
required features + dep graph + `generate` body). **Discovery = explicit registry
list** (`&[&dyn Driver]` held by the CLI); the `inventory` + `black_box init()`
hack is removed. **External/user plugin crates are out of scope for v1** (drivers
ship in-tree, curated).

## 10. Codegen pipeline
`parse → validate → resolve(→DeviceTree IR) → emit*`.
- **parse**: YAML → typed `Project` (refs normalized at deserialization,
  `$name` → `PinRef`/`ResourceRef`).
- **validate** (gates `compile`): unknown drivers, unresolved refs, `with:` type
  mismatches, pin/peripheral double-claims, cycles, feature/dep coherence. Emits
  span-aware `Diag`.
- **resolve**: builds the typed `DeviceTree` IR (instances, dep edges + access
  paths, `init_order`, `required_features`, flags).
- **emit** (pure `fn(&DeviceTree) -> Result<Artifact>`): Rust sources,
  `Cargo.toml`, wokwi `diagram.json`, JSON dump, DOT graph. A thin I/O layer
  writes `Artifact`s and records them in the ownership manifest.

## 11. Configuration model
Typed sectioned YAML — `espforge:` (name/platform/runtime), `esp32:` (peripherals),
`components:` (`using:` + `with:`), `devices:` (same). `using` selects a driver;
`with:` is deserialized into the driver's typed schema and validated up front.
References use `$name` (esphome-like, familiar). **App logic lives only in
`app.rs`** — YAML is structure-only, no declarative-step DSL. (Exception to
"no section renames": the v2 `esp32:` section adopts the v1 named-map shape and
field spellings — see §17.4. The `components`/`devices` list form is unchanged
from earlier v2 design.)

## 12. Runtime contract
Generated `Context { logger, delay, component!/device! accessors }` is the
**stable app-facing API**. Internally, `PeripheralRegistry` yields specific typed
peripherals **by value** into generated per-project `Components::new(...)` /
`Devices::new(...)` signatures (produced by the IR) — **no `RefCell`, no `Option`,
no `take().unwrap()`**. The compiler statically forbids double-claim. Embassy
`Stack<'static>` passed as a borrow; `needs_stack` flag (IR) is the single source
for whether it's threaded.

## 13. Validation & diagnostics
Single `validate` stage before `resolve`, gating `compile`. Errors are
**span-aware, structured `Diag`** (file:line:col + field path + fix hint) — not
bare `anyhow` strings (reserved for pipeline/I/O). `espforge validate`
subcommand runs the same stage and reports without emitting.

## 14. Testing strategy
Primary = stage-level unit tests + IR/token golden tests, all host-side and
hermetic. **Discipline: tests written when an actual bug is detected** (regression
tests), not speculatively. `espforge_examples` `cargo build` retained as a **CI
integration gate**. Mock-HAL runtime tests deferred to a post-v1 optional layer.

## 15. Migration strategy
Clean-slate **big-bang on new branch `espforgev2`**, built ground-up from a blank
sheet; v1 repo left intact. The v2 `esp32:` shape diverges from v1 user YAML
(D4), so existing v1 projects are **ported by hand** into the v2 contract (§17.4)
rather than carried over verbatim. Exit criterion: `espforgev2` reproduces example
outputs and passes `espforge validate` on all `espforge_examples`. CLI ships
**`create`** (alias **`setup`**), **`build`**, **`validate`**, and **`version`**
subcommands.

## 16. Alternatives considered (per decision)
- **D1:** one-shot scaffolder (A) rejected — regen machinery unjustified; runtime
  framework (C) rejected — fights owned-peripheral static wiring. Chose
  generator+maintainer (B).
- **D2:** multi-backend trait (B) rejected as speculative (Rust-coupled comp lib
  can't be reused cross-language); multi-backend-now (C) rejected — no named
  demand. Chose single backend + IR/multi-emitter.
- **D3:** two-tier collapse (B) rejected — loses real component/device distinction;
  pure graph (C) rejected — generality tax. Chose 3-tier typed.
- **D4:** flat single-list (B) rejected — loses ergonomic grouping; alt-sigil (C)
  rejected — abandons esphome familiarity. Chose typed sections + `$name`.
- **D5:** minimal monolith-extract (B) rejected — leaves config-as-IR coupling;
  heavy plan-phase (C) folded into A's resolve stage. Chose pure pipeline + IR.
- **D6:** catalog-driven (B) rejected — bespoke drivers don't fit templates;
  external plugins (C) rejected — no dynamic load on target. Chose single-file
  derive + explicit registry.
- **D7:** keep-13-crates (B) rejected — sprawl is a cost; one-mega-crate (C)
  rejected — breaks std/no_std wall. Chose 5 role-grouped crates.
- **D8:** `RefCell<Option>` take (A) rejected — runtime enforcement of
  compile-time-known fact; shared-borrow (C) rejected — sharing is component-tier.
  Chose move-by-value (B).
- **D9:** lazy validation (B) rejected — fail-late pain; validate-only-command (C)
  rejected — doesn't gate compile. Chose fail-fast validate stage + `validate` cmd.
- **D10:** e2e-only (B) rejected — slow/opaque; mock-HAL (C) deferred — larger
  investment. Chose stage + IR golden tests, bug-driven.
- **D11:** strangler (A) and per-driver port (C) reconsidered — user opted for
  clean-slate `espforgev2` big-bang. Chose ground-up rewrite on new branch.

## 17. `create` / `build` — the clean-and-jerk pair (derived from `/grill-me`)

The user workflow is a two-phase **clean-and-jerk**: `create` bootstraps a
project folder **once** from a curated template; `build` is the **repeatable**
re-emit on every YAML/`app.rs` edit. This split mirrors v1's `example`/`compile`
split but is grounded in the v2 drift-detecting manifest (ADR-001): `create`
lays down the source-of-truth inputs, `build` regenerates espforge-owned layers
and refuses to clobber user-edited (drifted) files.

### 17.1 `create` — bootstrap (runs once)

```
espforge create <example> [--name X] [--out DIR]
espforge create            # no <example> → interactive dialoguer picker
espforge setup <example>   # alias of create
```

- **Template source is baked-in at compile time** via `include_dir!` over
  `espforge-examples/examples` (v1 behaviour, kept). The example set is a
  **closed, explicit, versioned** set — no ambient filesystem discovery (Zen of
  espforge: explicit over implicit). This keeps "does this example exist?"
  a pure, testable map lookup (no IO), satisfying the v2 pipeline-purity ethos
  and the CI integration gate (ADR-010/011).
- **Unknown example name → error + exit.** No fuzzy "did you mean" picker; the
  set is closed.
- **Interactive fallback:** when `<example>` is omitted, a `dialoguer` `Select`
  lists the baked-in templates and an `Input` collects the project name. No
  full-screen TUI/ratatui — the interaction is a flat pick + a name, which does
  not justify a heavy dependency and would break scriptability/testability.
- **`--name` defaults to the example name** when omitted, and the resolved name
  is **always echoed back** to the user (no silent substitution). Folder =
  `<out>/<name>` (default `<out> = cwd`).
- **`create` copies assets only — it does not run the pipeline** (v1 `example`
  behaviour). It prints friendly, explicit next-steps: the exact `espforge build`
  invocation to run, where `app.rs`/`diagram.json` live, and what to edit.
- **Assets copied** (template tree shape preserved from v1):
  - `<example>/<example>.yaml` → `<name>/<name>.yaml` (the spec / source of truth)
  - `<example>/app/rust/app.rs` → `<name>/src/app.rs` (user-owned app logic)
  - `<example>/diagram.json` → `<name>/diagram.json` (wokwi, optional)

### 17.2 `build` — regenerate (repeatable)

```
espforge build [--project X --out DIR]
```

- **Arg-less form runs in `cwd`.** It discovers the spec by finding the YAML
  containing `espforge:` + `components:`/`devices:` (the spec named `<name>.yaml`
  left by `create`), sets `out = cwd`, then runs `scaffold()` (esp-generate) →
  `generate()` → `write()` (drift-detecting manifest). Path args become optional
  overrides for out-of-tree or multi-spec use.
- This makes the repeat invocation trivial: `cd <project> && espforge build`.

### 17.3 App-code collision (resolved)

`build` **generates `src/app.rs` itself** as a `SeedOnce` artifact
(`emit/rust.rs`): written only if absent, never clobbered. Therefore a template's
`app/rust/app.rs` lands at `src/app.rs` first (during `create`), and `build`
subsequently **leaves it untouched**. Consequence: every template's `app.rs`
**must be v2-authored** — it uses the v2 `component!(ctx, name)` /
`device!(ctx, name)` macros (`emit/rust.rs`) and the embassy `async` signatures
when `runtime: embassy`. Templates are **not** auto-converted from v1; they are
ported by hand into the v2 shape. "Keep all aspects of v1 examples" means keep
the v1 **tree shape and feel**, not the v1 schema.

### 17.4 v2 YAML contract (explicit over implicit)

Adopted from the grilling: **v1 named-map `esp32:` shape + v2 `using:` driver
kinds + v2 list-form `components`/`devices`**. No implicit behaviour.

```yaml
espforge:
  name: blink
  target: esp32c3          # v1 `platform` → v2 `target`
  runtime: blocking        # or embassy; v2 also supports `alloc: true`

esp32:                     # NAMED MAP, v1 shape (explicit key per resource)
  gpio:
    gpio2: { pin: 18, direction: output }
    gpio9: { pin: 9, direction: input }
  i2c:  { i2c0: { i2c: 0, sda: 6, scl: 7, frequency_kHz: 100 } }
  spi:  { spi2: { spi: 2, sck: 3, mosi: 4, miso: 0, frequency_kHz: 10000 } }
  uart: { uart0: { uart: 1, tx: 6, rx: 5 } }
  wifi: { ssid: Wokwi-GUEST, password: "", auth: open }
  psram: { mode: octal }
  heap: { size: 73000 }

components:                # LIST form, explicit id, v2 lowercase driver kinds
  - id: red_led
    using: led
    with: { pin: $gpio2, active_low: false }
devices:
  - id: screen
    using: ssd1306
    with: { bus: $i2c0, reset: $GPIO16, dc: $GPIO5 }
```

Contract specifics:
- **`esp32:` is a named map** keyed by resource name (`gpio2`, `i2c0`). Field
  names are the v1 spellings: `pin`, `direction`, `i2c`/`spi`/`uart` peripheral
  index, `sda`/`scl`, `tx`/`rx`, `frequency_kHz`, `ssid`/`password`/`auth`,
  `mode` (psram), `size` (heap). This overrides the earlier v2 sequence form
  (`gpio: [{ gpio: 18 }]`) at the model level — `create` and `build` share the
  same parser, so this is a global parser change, not template-only.
- **`$ref` = the `esp32` map key** (`$gpio2`). This matches v1 behaviour and
  removes the earlier v2 inconsistency where gpio used `$GPIO18` but buses used
  `$i2c_master`. Peripherals are referenced by their map key uniformly.
- **`direction` is accept-and-echo** for v1: carried into the IR and echoed, but
  not yet enforced (e.g. an `led` pin need not yet be validated as `output`).
  Enforcement is a later `validate` enhancement.
- **`esp32` is schema-complete** even where emit is lazy: `psram`/`heap`/`wifi`
  are accepted so the schema matches v1; codegen consumes them when the
  corresponding driver is present (esp-generate `-o wifi`, embassy/alloc flags).
- **`components`/`devices` stay the v2 LIST form** `{ id, using, with }` — the
  minimal blast radius. `id` is the explicit name, used as the `$ref` source for
  other instances and as the generated struct field name (`emit/rust.rs`).

### 17.5 Migration notes (current branch state)

- Rewrite the in-repo v2 templates from the old sequence form to §17.4:
  `espforge-examples/examples/blink/blink.yaml`, `.../display/ssd1306.yaml`, and
  `.../blink/broken.yaml` (the latter must still exercise the validation path).
- `Esp32Section` in `espforge-model/src/project.rs` changes from `Vec<GpioPin>`
  to a name-keyed map; `pipeline.rs`/`validate` carry the name + direction
  through to `IR::Peripheral`. Shared by `create` and `build`.

## 18. Decision ledger (from `/grill-me`)

1. `create` is template-driven; it feeds examples into the same build pipeline
   (no parallel template-copy code path — keeps the v2 pipeline single-source).
2. CLI args primary; `dialoguer` interactive picker on missing/unknown example;
   **no** fuzzy "did you mean" and **no** ratatui.
3. Templates baked in at compile time via `include_dir!`; unknown name → error +
   exit; `--name` defaults to the example name and is always echoed back.
4. `create` copies assets only + prints friendly next-steps (v1 `example`
   behaviour), does **not** emit.
5. `build` runs arg-less in `cwd`, discovering the spec; `--project`/`--out` are
   optional overrides.
6. `esp32:` uses the v1 named-map shape + v2 `using:` kinds + `$key` refs;
   `direction` accept-and-echo; `esp32` schema-complete (psram/heap/wifi).
7. `components`/`devices` keep the v2 list form with explicit `id`.
8. Templates are v2-authored (hand-ported), not auto-converted from v1; the
   example tree shape + feel is preserved, the schema is not.

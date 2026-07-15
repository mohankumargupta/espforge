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

### 2b. Networking model (ADR-012, decided 2026-07-16)

Networking follows **esphome's shape**, not the per-instance-claim shape used
for buses. There is **no `tcp` component** — the TCP/IP `Stack` is implicit
infrastructure built once from the top-level `esp32.wifi` block, exactly as
esphome's `http_request:` presumes a global network link and never references a
named `wifi:` instance by id.

- **`esp32.wifi` is a top-level *peripheral block*** carrying `ssid`/`password`/
  `auth` (already in the schema, §17.4). It is **not claimed by any instance**;
  the emitter consumes it directly when `needs_stack` is set. This keeps the
  `claimed_by` invariant clean (no instance owns the singleton network resource).
- **`http` is a software-service `Component`** (`using: http` in `components:`),
  declared like any other component but asserting the cross-cutting flags
  `is_embassy`, `has_wifi`, `needs_stack`, `has_alloc`. It **claims no
  peripheral** and takes no `with: { bus: $wifi }`.
- **HW vs SW distinction is user-invisible (ADR-013):** no separate `services:`
  YAML key; a single unified `components:` namespace and a single unified
  generated `ctx.components.*`. The `nature` is **derived** — `Service` iff the
  instance declares no `pins`/`peripherals` (only flags) — and exists only
  *internally*: as a `espforge-runtime/src/services/` code folder and as a
  validator rule (`Service ⇒ no pins/peripherals`).
- The **Stack singleton** (one `embassy_net::Stack` + its `StackResources`,
  spawned `connection` + `net_task` tasks, `wait_config_up()`) is built **inline
  in generated `main.rs`** from the `esp32.wifi` block when `needs_stack` is set.
  It is exposed to components as a `&'static Stack` (emitter-named global,
  e.g. `NET_STACK`).
- **`http`'s runtime wrapper** (`espforge-runtime::components::Http`) receives
  `&'static Stack` as an explicit constructor arg (`Http::new(NET_STACK)`) — same
  `ctor`/move-by-value convention as `ssd1306`/`i2c`. Internally it wraps
  `edge_http` (`edge_http::io::client::Connection`) and hides the buffer/read-loop
  boilerplate behind ergonomic `async get/post -> Result<String>`. The app never
  names `edge_http`, `Connection`, or `Stack`.
- **Bridge:** `edge_nal_embassy` provides the `edge_nal::TcpConnect + Dns` impl
  for `&embassy_net::Stack` that `edge_http` requires (one feature-gated runtime
  dep).
- **WiFi crate:** the network path targets **`esp-radio` / `esp_radio::wifi`**
  (current/maintained line, matching the canonical esp-hal 1.1 example), *not*
  the older `esp-wifi`. The generated Cargo `esp-wifi = "*"` line is superseded
  by `esp-radio` for the network path. `has_wifi` stays the crate-agnostic flag
  name.
- **Scope:** first implementation = **plaintext HTTP only** (port 80). TLS/HTTPS,
  `mqtt`, and `websockets` are **planned future work** (same Stack + `edge_nal`
  bridge; `edge-mqtt`, `edge-ws` / `tokio-tungstenite` + `rustls`/esp-tls for
  HTTPS) — not non-goals, just deferred. UDP is out of scope.
- **Validation guards:** if `http` is present but `esp32.wifi` is absent, `validate`
  fails with a span-aware `Diag`. If `runtime: blocking` is declared with `http`,
  `resolve` **auto-upgrades to Embassy** (any instance asserting `is_embassy`
  wins), so no blocking network path exists.

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
- Within `espforge-runtime/src/components/`, hardware drivers and software-
  service drivers are further separated into `components/` and `services/`
  subfolders **internally only** (ADR-013) — this is code organization, not a
  user- or app-visible distinction, and does not change the unified `components:`
  YAML key or the generated `ctx.components.*` accessor.

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
list** (`&[&dyn Driver]` held by the CLI, assembled automatically — see §9b);
the `inventory` + `black_box init()` hack is removed. **External/user plugin
crates are out of scope for v1** (drivers ship in-tree, curated).

### 9b. Automatic registry assembly (build-script codegen)

The registry list in §9 is still *explicit* (no link-time discovery) but its
**assembly is generated at compile time** so adding a driver file needs no edit
to any `mod.rs`. This is the same pattern `tonic-build`, `protobuf-build`,
`sqlx`, `bindgen`, and `capnproto` use: a `build.rs` inspects the source tree and
emits a `.rs` into `OUT_DIR` that the crate `include!`s.

**Mechanism.**
1. `espforge-bindings/build.rs` walks `src/components/` and `src/devices/`.
   Every `*.rs` that is not `mod.rs` is a driver module; its filename stem is the
   module name (`led`, `i2c`, `ssd1306`).
2. For each driver module, the generated file emits `pub mod <name>;` and a
   reference to that module's driver const.
3. Each driver module **exports a fixed-name const** — `pub const DRIVER:
   &'static dyn Driver = &<NAME>;` — so the generator never has to guess the
   per-driver const name (e.g. `LED`, `I2C`). This is the load-bearing
   convention; it removes the fragile "uppercase the filename" step.
4. Generated output (e.g. `OUT_DIR/components_gen.rs`):
   ```rust
   pub mod led;
   pub mod i2c;

   use espforge_model::driver::Registry;
   pub fn registry() -> Registry {
       Registry::new(&[led::DRIVER, i2c::DRIVER])
   }
   ```
   `src/components/mod.rs` collapses to a single line:
   `include!(concat!(env!("OUT_DIR"), "/components_gen.rs"));`
5. `build.rs` emits `cargo:rerun-if-changed=src/components` and
   `=src/devices` so adding/removing a file regenerates on the next build.

**Why this shape (not `inventory`/`linkme`).** Discovery happens at *compile
time* in inspectable source under `target/.../out/`, not via linker sections
(ADR-006: "no link-time discovery magic"). Zero new runtime dependencies; the
registry stays a plain `&[&dyn Driver]`, so all consumers (`emit/rust.rs`,
`pipeline.rs`) are unchanged and `registry().all()` is still a static slice.

**Trade-offs.**
- Adds a `build.rs` step; `rerun-if-changed` keeps it a no-op on unchanged builds.
- The generated file is derived (lives in `target/`, not the repo) — standard.
- The `pub const DRIVER` export is a hard convention per driver module.

**Day-to-day.** Drop `src/components/button.rs` (declaring `pub static BUTTON`
and `pub const DRIVER = &BUTTON`) → `button` is in the registry on next build,
no `mod.rs` touch. Remove the file → it leaves the registry.

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
for whether it's threaded. The Stack is built **inline in `main.rs`** from the
top-level `esp32.wifi` block (not claimed by any instance) and exposed to
components as a `&'static Stack` (emitter-named global `NET_STACK`), per ADR-012.

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
  and the CI integration gate (ADR-010/011). The list of example names is
  **derived** from the embedded tree at runtime (every leaf dir containing a
  spec), not hand-maintained — adding a template needs no code change.
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
- **Assets copied** (template tree shape preserved from v1, including the
  numbered-category folder convention — `01.Basics/blink`, `06.Displays/display`,
  …). The example key is the **leaf folder name**; `create blink` resolves to
  `01.Basics/blink` regardless of category (v1 resolution behaviour):
  - the example's spec (the embedded `.yaml` containing `espforge:`) →
    `<name>/<name>.yaml` (the spec / source of truth). The source filename is
    decoupled from the example key, so `display`'s spec may be `display.yaml`.
  - `app/rust/app.rs` → `<name>/src/app.rs` (user-owned app logic)
  - `diagram.json` → `<name>/diagram.json` (wokwi, optional)

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

## 19. Conditional compilation — only used deps are compiled (grilling session)

**Problem.** A generated firmware project should compile *only* the
`espforge-runtime` modules and external crates it actually uses (e.g. `helloworld`
must not pull the `led`/`ssd1306` runtime code or their deps). As of the session
start this was **not implemented**: `espforge-runtime` unconditionally
`pub mod`-ed every module with no `[features]` and no optional deps, and
`emit_cargo_toml` emitted a fixed dep set (only `embassy`/`alloc` were
conditional). The `Driver::required_features()` hook existed but `emit`
ignored `ir.required_features`.

**Scope decision.** Conditionality applies **only to the generated firmware
project** (`espforge-runtime` features + the generated `Cargo.toml` deps). The
host toolchain (`espforge`, `espforge-bindings`) stays monolithic — it compiles
all drivers because it is a compiler over a closed driver set; this costs
nothing on-device.

### 19.1 Mechanism

- **One crate, feature-gated.** Keep a single `espforge-runtime` leaf (no
  per-capability crates — that would explode the published-crate count and break
  the curated in-tree-driver model, ADR-006/§9). Each capability module is
  gated: `components/mod.rs` / `devices/mod.rs` use
  `#[cfg(feature = "led")] pub mod led;` etc. The `feature` name equals the
  module's folder/stem name (reuses the §9b `build.rs` walk convention).
- **External driver crates are optional deps of `espforge-runtime`.** e.g.
  `[dependencies.ssd1306] optional = true` + `ssd1306 = ["dep:ssd1306"]`. The
  runtime re-exports whatever the app touches under `#[cfg(feature = "ssd1306")]`
  (`pub use ssd1306::…`). The generated project depends *only* on
  `espforge-runtime` (with features), never directly on the upstream crate. This
  keeps the app manifest tiny and identical in shape regardless of drivers, and
  the compiler only pulls the external crate when its feature is on.
- **Per-driver cost is small + explicit.** Adding a real driver needs: the
  module file, **one `[features]` line** in `espforge-runtime/Cargo.toml`, **one
  cfg-gated `pub mod` line** in `mod.rs`, the §9b `pub const DRIVER` export, and
  `runtime_features()`. The `build.rs`-generated bindings registry (§9b) stays
  untouched. Cargo's features are static, so the `Cargo.toml`/`mod.rs` lines are
  unavoidable (a second `build.rs` to auto-generate `mod.rs` cfg-lines is
  rejected as excess machinery).

### 19.2 Driver trait split

The old `Driver::required_features()` conflated two concepts. They are split:

- **`fn runtime_features(&self) -> &[&str]`** (new) — returns the
  `espforge-runtime` **module feature names** this driver needs. Default
  `[self.kind()]`. The `kind` is the canonical feature name; an override is
  permitted but discouraged (rare case: two `using:` drivers sharing one feature
  set / external dep — the union dedups).
- **`DriverFlags`** (existing, `driver.rs`) carries the cross-cutting
  **project-level** flags (`has_alloc`, `has_wifi`, `needs_delay`, `is_embassy`,
  `needs_stack`) via `ir.flags`. These map to project deps (see §19.3), not to
  `espforge-runtime` module features. **`is_embassy` must also be added to
  `SpecFlags`** (the per-driver catalog flags) and consulted in `resolve`, so a
  software-service component like `http` can force Embassy even when the YAML
  says `runtime: blocking` (auto-upgrade, ADR-012). Currently `DriverFlags.
  is_embassy` exists but the resolve loop only reads `SpecFlags.{has_wifi,
  needs_stack}` — this gap must be closed at implementation time.
- **`required_features()`** is repurposed/dropped to remove the ambiguity.

### 19.3 Manifest emission

`emit_cargo_toml` is rewritten to be driven purely by `ir.flags` plus the
**union of `runtime_features()`** over `ir.instances`:

- `is_embassy` → `embassy-executor` + esp-hal `"embassy"` feature. Asserted by
  `http` (ADR-012); `resolve` auto-upgrades from `runtime: blocking`.
- `has_alloc` → `embedded-alloc`.
- `esp-hal` → pinned to `1` (NOT `"*"`), else the resolver falls back to the
  old esp-hal 0.17 cluster.
- `has_wifi` → `esp-radio = "1"` (+ `esp-radio/wifi` feature). *(supersedes the
  older `esp-wifi = "*"` line in `emit/rust.rs`; the network path targets the
  maintained `esp_radio::wifi` API, ADR-012.)*
- `needs_stack` → `embassy-net = "0.9"`. *(new)*
- network software-services (`http`) → `edge-http = "0.8"`, `edge-nal = "0.7"`,
  `edge-nal-embassy = "0.9"`, `embassy-time = "0.5"` (feature-gated runtime
  deps; bridge `embassy_net::Stack` → the `edge_nal` traits `edge_http`
  requires, ADR-012). **All pinned, not `"*"`** — `edge-http` 0.4's
  `Connection::new(buf, &Stack, addr)` blanket-impl API was replaced in 0.8 by
  `edge_nal_embassy::{Tcp, Dns}` wrapper types requiring a TCP buffer pool
  (`TcpBuffers`); the runtime `Http` wrapper (`services/http.rs`) is written for
  this pinned cluster. Do not loosen these to `"*"` or the resolver re-locks the
  incompatible old cluster (esp-hal 0.17 + embassy-net 0.5 + edge-http 0.4).
- drivers → `espforge-runtime = { …, features = [<union>] }`.
- `Logger` / `Delay` stay unconditionally compiled (shared, negligible);
  `needs_delay` remains a marker, not a separate dep.
- The flag→dep mapping is **centralized in the emit step**, not scattered into
  driver declarations.

### 19.4 Plumbing

`emit()` computes the feature-union in `BTreeSet<String>` (deduped + sorted for
**deterministic manifests**, which matters for drift detection, §5.1) and passes
it **explicitly** to `emit_cargo_toml(ir, &rt_features)`. It is *not* stored back
into `DeviceTree` (the IR stays driver-agnostic, ADR-006). Both the published
`"0.1.0"` form and the `ESPFORGE_USE_LOCAL` path-dep form carry `features = […]`;
when the set is **empty the dep is emitted bare** (`espforge-runtime = "0.1.0"`),
matching the current minimal-project output.

### 19.5 Verification

The esp32c3 uses a RISC-V target (`riscv32imc-unknown-none-elf`) that is part of
the **stable** Rust toolchain — no esp-patched fork needed for a compile check
(the esp fork only adds the `esp32c3` bare-metal alias + extra SOCs). In this
repo's environment the stable toolchain plus `riscv32imc-unknown-none-elf` are
installed, so a generated `esp32c3` firmware project **can** be cross-compiled
on the host to prove conditionality.

Two complementary checks:
- **Golden/assertion tests on the emitted `Cargo.toml`** (host-side, hermetic,
  matches the §14 test discipline) — fast first-line guard:
  - `helloworld.yaml` → `espforge-runtime = "0.1.0"` (no `features`, no
    `ssd1306`/`esp-wifi` anywhere).
  - `display.yaml` → `espforge-runtime = { …, features = ["ssd1306"] }`.
- **Real cross-compile** of generated projects for `esp32c3` against
  `riscv32imc-unknown-none-elf` as a CI integration gate. Because cargo only
  pulls optional deps whose feature is enabled, a `helloworld` build that
  succeeds **without** compiling the `ssd1306` module/external crate is the
  definitive proof that unused deps are excluded. (Note: `ESPFORGE_USE_LOCAL`
  path-deps point at the workspace `espforge-runtime`; ensure the workspace
  `[workspace]` exclusion in the generated manifest doesn't fight the local
  path resolution.)

### 19.6 Status

Networking design **decided** (ADR-012, grilling session 2026-07-16): no `tcp`
component; `esp32.wifi` is a top-level block; Stack built inline in `main.rs`;
`http` is a wrapped runtime component (`ctx.components.http`) over `edge_http` +
`edge_nal_embassy` on an `esp_radio`/`embassy_net` stack; plaintext only (HTTPS/
mqtt/websockets deferred to future work, same Stack + `edge_nal` bridge). Not yet
implemented — open `implement` items: add `is_embassy` to `SpecFlags` + wire into
`resolve`; swap `esp-wifi` → `esp-radio` in `emit/rust.rs`; add `Http` runtime
component + manifest deps.

# espforge v2 — Design (compiled from ADR log)

This document is the compiled output of an 11-decision first-principles redesign
session (`/grill-with-docs`). It is ground-up: it does not inherit the v1
13-crate layout. The authoritative record of *why* is the ADR log under `adr/`.

Status: design only. No architecture or code has been produced yet.

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
`app.rs`** — YAML is structure-only, no declarative-step DSL. No section renames.

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
sheet; v1 repo left intact. User YAML unchanged (D4) so existing projects carry
over without edits. Exit criterion: `espforgev2` reproduces example outputs and
passes `espforge validate` on all `espforge_examples`. CLI ships **`validate`**
and **`version`** subcommands.

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

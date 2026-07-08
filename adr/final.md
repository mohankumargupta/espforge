# ADR-001 — Problem frame & source of truth

**Status:** accepted

**Decision.** espforge is a YAML/`app.rs`-driven generator + maintainer. Source of
truth = {project YAML, `app.rs`, `dependencies.toml`, optional `.cargo/config.toml`
override, optional `diagram.json` override}. All other project files are generated.

**Boundary enforcement.** espforge emits an **enforcement-grade** ownership
manifest (input files + checksums, set of owned files) and a human `README.txt`.
Layered files (`.cargo/config.toml`, `diagram.json`): if a user copy exists in the
source-of-truth dir, it **fully replaces** espforge's generated base (binary
ownership, no merge). Regeneration is idempotent and drift-detecting; a checksum
mismatch on an owned file → espforge refuses rather than clobbers.

**Drivers.** Target user edits a declarative spec, not boilerplate. ESP32
peripherals are owned singletons best wired statically. The existing regen
machinery is only justified if regeneration is a first-class operation.

**Consequences.** Generated files never hand-edited; `Cargo.toml` generated, user
deps via `dependencies.toml`; regeneration idempotent + drift-detecting.
# ADR-002 — Codegen backends

**Status:** accepted

**Decision.** espforge has exactly **one backend: Rust firmware** (blocking +
embassy are *modes* of that backend, not separate backends). The architecture
provides a single inspectable **IR** with **multiple emitters** (Rust sources,
`Cargo.toml`, wokwi `diagram.json`, JSON dump, DOT graph) — these are *artifacts of
one backend*, not language backends. A **multi-language backend** (Zig / MicroPython
/ C) is explicitly a **non-goal for v1**; no `Backend` trait is erected.

**Drivers.** No concrete second backend named with a near-term driver. A
Rust-coupled component library (wraps `esp_hal`) cannot be reused by another
language, so a present-day `Backend` trait would encode a fiction. The IR remains
the future seam if a real second backend ever appears.

**Consequences.** Emitters are `fn(&DeviceTree) -> Result<Artifact>`; adding an
artifact type cannot break the Rust emitter.
# ADR-003 — Core domain model & ubiquitous language

**Status:** accepted

**Decision.** espforge's core model is a **3-tier typed spine**: Peripheral (raw
hardware) → Component (reusable named capability with an API — hardware-backed *or*
software-service) → Device (terminal high-level driver consuming components ± pins).
Wiring forms a DAG: components may be consumed by components and devices; devices
are terminal (consumed only by the app). Every instance is typed
(component_kind / device_kind / resource kind), not `driver: String` + `Value` — so
validation and dependency ordering become structural. A single inspectable **IR
(DeviceTree)** is the artifact all emitters read.

**Ubiquitous language.**
- Peripheral — raw ESP32 hardware resource (pin, I2C/SPI/UART bus, WiFi)
- Component — reusable named capability with an API; hardware-backed or
  software-service (http, mqtt, websockets, voice_control, accelerometer)
- Device — terminal high-level driver consuming components ± pins
- Instance — one named occurrence of a component/device in a project
- ResourceRef / PinRef — typed reference value object to a named resource
- Project — the whole spec: metadata + peripherals + components + devices + app
- IR / DeviceTree — validated intermediate representation all emitters read

**Boundary rule asserted.** Devices are terminal — a device may not be consumed by
another device, only by the app. (Deliberate simplification vs esphome, keeps the
DAG acyclic.)

**Drivers.** The tier model mirrors how embedded firmware is built and how
datasheets describe hardware. B's two-tier collapse loses a real distinction; C's
pure graph is a generality tax.
# ADR-004 — Configuration model

**Status:** accepted

**Decision.** Typed sectioned YAML — `espforge:` (name/platform/runtime),
`esp32:` (peripherals), `components:` (`using:` + `with:`), `devices:` (same).
`using` selects a driver; `with:` is deserialized into the driver's typed schema
and validated up front (not re-parsed as raw `Value`). References use `$name`,
normalized to typed `PinRef`/`ResourceRef` at deserialization (esphome-like,
familiar). **App logic lives only in `app.rs`** — YAML is structure-only, no
declarative-step DSL. No section renames.

**Drivers.** The sectioned model maps 1:1 to the 3-tier spine users think in.
Flattening to a single list loses ergonomic grouping; alternate sigils trade away
esphome familiarity for marginal safety already provided by typed refs. Keep the
explicit-over-implicit philosophy: addresses, active-low, frequencies stay in YAML.

**Consequences.** Each component/device declares a schema; `with:` validated at
parse time (fixes fail-late validation). Config model is purely declarative.
# ADR-005 — Codegen architecture

**Status:** accepted

**Decision.** Generation is a **pure staged pipeline**: `parse → validate →
resolve(IR) → emit*`. The dedicated **`DeviceTree` IR** (produced by `resolve`)
carries: project metadata, typed peripherals, typed instances (resolved `with:`
params + dependency edges with kinds/access-paths), `init_order`,
`required_features`, and flags (is_embassy, has_alloc, has_wifi, needs_delay,
needs_stack). **All emitters are pure `fn(&DeviceTree) -> Result<Artifact>`** where
`Artifact` is an in-memory file (path + content); a thin I/O layer writes them and
the ownership manifest records them. Emitters read only the IR, never the raw
config.

**Drivers.** The single documented pain is the untestable monolith and fail-late
validation. A pure staged pipeline with an IR directly fixes both: stages are
pure `fn`s, validation becomes a stage before codegen. The IR is the seam ADR-002's
multi-emitter needs. Keeping `EspforgeConfiguration` as the IR leaks parse-time
concerns into codegen.

**Consequences.** Emitters pluggable and isolated; parse/codegen/config unit-
testable (ADR-010); IR is valid-by-construction after validate (ADR-009).
# ADR-006 — Extension / plugin model

**Status:** accepted

**Decision.** Drivers are declared **one module/file each** via a derive macro
carrying: typed config, `using` name, required features, dependency graph, and a
`generate` body emitting a code fragment. This collapses the current 5-files-across-
4-crates spread to a single declaration. **Discovery = explicit registry list**
(`&[&dyn Driver]` of built-in drivers held by the CLI); the `inventory` +
`black_box` `init()` hack is removed — most debuggable, no link-time magic.
**External/user plugin crates (out-of-tree dynamic discovery) are out of scope for
v1**; drivers ship in-tree and curated.

**Drivers.** The 5-file spread is pure structural overhead with no benefit — config
struct, plugin logic, and runtime impl are one concept. Catalog-driven drivers (B)
can't express bespoke init logic (ssd1306). External crates (C) are wrong for
embedded: no runtime dynamic loading on target, host-side discovery adds a version
matrix for marginal benefit when the driver set is curated.

**Consequences.** Per driver = 1 declaration module; registry is an explicit list.
# ADR-007 — Workspace & crate layout

**Status:** accepted

**Decision.** Workspace collapses from 13 kind-split crates to **5 role-grouped
crates**:
- `espforge` — CLI binary + parse/resolve/emit orchestration (host/std).
- `espforge-model` — the `DeviceTree` IR, the `Driver` trait, and the explicit
  registry **types** (host/std; depends on neither host nor target).
- `espforge-bindings` — in-tree `generate` impls + the driver registry list
  (host/std). *Devicetree-bindings analogy: contract → codegen glue.*
- `espforge-runtime` — `no_std` runtime implementations of each capability
  (`LED`, `SSD1306Device`, …), **split into distinct `components` and `devices`
  modules**. *Runtime analogy; preserves the component/device distinction esphome
  blurs.*
- `espforge-examples` — sample projects.

**Dependency rule (hard).** `espforge-runtime` depends only on `esp-hal`/
`embedded-hal` (leaf); `espforge-model` depends on neither host nor target; host
crates (`espforge`, `espforge-bindings`) reference `espforge-runtime` *only by
name/path inside emitted token streams* — never link it into the host build. **No
cross-boundary cycles.**

**Runtime module split.** Unlike esphome, `espforge-runtime` keeps **separate
`components` and `devices` modules**, mirroring the three-tier domain spine
(ADR-003). A component capability (I2cDevice, LED, http) lives under `components`;
a terminal device (ssd1306, ili9341) lives under `devices`. This makes the
structural distinction that D3 encodes (devices are terminal) visible in the
runtime layout itself.

**Per-driver file count:** 2 (generate-impl in `espforge-bindings`, runtime-impl in
`espforge-runtime` under its `components` or `devices` module), down from today's 5.

**Drivers.** The dominant constraint is the host/target (std/no_std) wall; a single
mega-crate mixes them and leaks host deps into the target. Grouping by *role*
(model / host-codegen / target-runtime) shrinks the surface and lands the D6
single-declaration cleanly. Keeping the 13-crate kind-split keeps the sprawl that
is a documented cost.
# ADR-008 — Runtime contract & pin ownership

**Status:** accepted

**Decision.** Runtime pin/peripheral ownership uses **move-by-value (B)**:
`PeripheralRegistry` yields the *specific* typed peripherals each component needs,
by value, into generated per-project `Components::new(...)` / `Devices::new(...)`
signatures produced by the IR. **No `RefCell`, no `Option`, no `take().unwrap()`.**
The typed IR (ADR-005) moves each peripheral exactly once; the compiler *statically*
forbids double-claim (a YAML double-claim surfaces as a generated-code compile
error, and is caught earlier by D9's validate stage). Embassy `Stack<'static>` is
passed as a borrow alongside owned peripherals.

**App-facing `Context` stays stable:** `{ logger, delay, component!()/device!()
accessors }` — app code shape unchanged even though internal wiring signatures are
generated per-project.

**Bus-sharing stays at the component tier:** an `I2cDevice`/`SpiDevice` component is
shared by reference by many devices; the registry itself need not support sharing.

**Scope of assurance.** Sound for the current esp-hal peripheral model (owned,
`'static`, movable types). A future borrowed-only HAL peripheral would degrade to
`RefCell<Option>` for that one peripheral only, not the whole system.

**Drivers.** A's `RefCell<Option>` + `take().unwrap()` is runtime enforcement of a
compile-time-known fact (each pin claimed by exactly one component, known at codegen
via the IR). B makes invalid states unrepresentable. C (shared-borrow at registry)
is unnecessary because bus-sharing already lives at the component tier.
# ADR-009 — Validation & diagnostics

**Status:** accepted

**Decision.** A single **`validate` stage runs before `resolve`/`emit`** and gates
`compile`. It checks: unknown drivers, missing/unresolved `$name` refs, `with:`
type mismatches, **pin/peripheral double-claims** (catches ADR-008-B's risk at YAML
time), dependency cycles, and required-feature/dependency coherence. Config errors
are **span-aware, structured `Diag`** (file:line:col + field path + fix hint), not
bare `anyhow` strings; `anyhow` is reserved for pipeline/I/O failures only. A
**`espforge validate`** subcommand runs the same stage and reports without emitting.

**Drivers.** The IR (ADR-005) exists to be the trustworthy artifact emitters
consume; if invalid config can reach `resolve`/`emit`, the IR can't be trusted and
emitters must defensively `unwrap` (today's late, location-less failures). Span-
aware `Diag` is the point of a user-facing generator. This also catches ADR-008-B's
double-claim risk cheaply. `validate` as a subcommand shares the same stage without
emitting; validation must gate `compile`, not be optional.

**Consequences.** IR is valid-by-construction; emitters and move-by-value runtime
stay clean.
# ADR-010 — Testing strategy

**Status:** accepted

**Decision.** Primary testing = **A**: stage-level unit tests on the pure `fn`s
(`parse`/`validate`/`resolve` → `DeviceTree`) + IR/token golden tests, all host-side
and hermetic. **Discipline: tests are written when an actual bug is detected**
(regression tests), not speculatively — the pure-pipeline design makes this cheap to
do on demand. `espforge_examples` `cargo build` is retained as a **CI integration
gate** (catches target-compile breakage) but is not the primary test. Mock-HAL
runtime tests (C) deferred to a post-v1 optional layer.

**Drivers.** A is the direct dividend of the ADR-005 pure pipeline: pure `fn`s are
trivially testable and the IR is the perfect assertion target (parse YAML → assert
`DeviceTree`; run emitter → assert source) without filesystem or target. Example
builds (B) are too slow/opaque to be primary. Mock-HAL (C) needs a mock `esp-hal`
surface and only tests runtime, not codegen — valuable later, not a v1 blocker.

**Consequences.** All A tests run on host (std); target crates tested via CI example
builds or future mock-HAL, never linked into host unit tests.
# ADR-011 — Migration strategy

**Status:** accepted

**Decision.** Migration is a **clean-slate big-bang on a new branch `espforgev2`**,
built ground-up from a blank sheet — *not* an in-place strangler on the existing
13-crate repo. The old repo is left intact (continues as v1); `espforgev2` is the
fresh implementation of ADR-001–010. **User YAML is unchanged** (ADR-004: sections +
`$name` + `app.rs` identical), so existing projects carry over without edits; the
cost of the big-bang is borne entirely internally (no dual-codebase maintenance in
one binary). The rewrite's exit criterion: `espforgev2` must reproduce the example
outputs and pass `espforge validate` on all `espforge_examples`.

**CLI surface includes `validate`** (ADR-009) **and `version`** (prints the espforge
version) as first-class subcommands.

**Drivers.** User YAML is the only external contract; a new engine is invisible to
users as long as it emits the same project tree shape. A strangler (A) and per-driver
port (C) were considered; user opted for the clean-slate rewrite to avoid dragging
the old `inventory`/5-file structure along. Big-bang risk is acceptable because the
YAML contract is stable and no live dual-codebase is maintained in one binary.

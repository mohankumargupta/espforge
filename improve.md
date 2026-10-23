# Architecture improvements — top 3

Goal: put code where it logically belongs so the workspace is easier to
navigate, test, and extend. No behaviour change: `check` / `build` / `gen`
accept and emit exactly what they do today.

Workspace for reference (member → role today):

- `espf/` — ESPF parser, values, `context::RuntimeContext`, diagnostics, writer
- `espforge-options/` — option schema, resolve, `# espforge` section reader
- `espforge-model/` — `Meta`, `Project`, `Artifact`/`Asset`, `Ownership` (160 lines)
- `espforge-bindings/` — typed wrappers for `<*_component>`, driver dispatch
- `espforge-emit/` — template render + manifest + `Hardware` spec structs
- `espforge/` (CLI) — `commands/{pipeline,frontend,build,check,new,…}`,
  `render.rs` (help text), `sections/`

How to work through this doc: do items in order (1 → 2 → 3). After each
item run `cargo test --workspace` and
`cargo build -p espforge`. Keep public CLI output byte-identical unless the
item says otherwise.

---

## 1. Break up `espforge/src/commands/pipeline.rs` (1,748 lines)

Status: 1a ✅ 1b ✅ (2026-10-24) — 1c–1f still to do.

### Problem

`pipeline.rs` is a god module inside the CLI crate. It currently owns four
unrelated jobs:

| Job | Functions | Approx. lines |
| --- | --- | --- |
| Source discovery | `Source`, `from_file`, `find_project`, `resolve_source` | ~45–100 |
| Validation orchestration | `Invalid`, `Checked`, `validate`, `unsupported_section`, `buildable` | ~100–180, 1048+ |
| Hardware lowering (file → `Hardware`) | `hardware()` + local drafts `I2cBusDraft`, `SpiBusDraft`, `SpiDeviceDraft`, `UartDraft`, `UartCompDraft` | ~323–1040 (~700 lines) |
| Per-bus parsing + pin-conflict checks | `uart_peripheral_name`, `uart_attr_peripheral`, `i2c_peripheral_name`, `parse_khz`, `wifi_spec`, `is_rust_ident`, `plain`, plus inline GPIO/LED/button/I2C/SPI/UART loops and 5+ conflict-check blocks | spread through `hardware()` |

Symptoms:

- Domain logic lives in the CLI: `hardware()` builds `Hardware`,
  `LedSpec`, `I2cSpec`, … but those structs are defined downstream in
  `espforge-emit/src/context.rs` (lines 46–147). The dependency points the
  wrong way: CLI → emit for domain types.
- `build.rs` (`execute`, ~586 lines) and `check.rs` (`execute`) both call
  into `pipeline::` for the same steps, so pipeline edits ripple into two
  commands plus their tests.
- The per-bus blocks cannot be unit-tested in isolation (e.g. “UART pin
  conflict” needs a full `Checked` fixture).

### Target

```text
espforge-model/src/hardware.rs        # LedSpec, ButtonSpec, I2cSpec,
                                      # SpiBusSpec, SpiSpec, UartSpec,
                                      # WifiSpec, Hardware (moved, unchanged)
                                      # + is_rust_ident helper

espforge/src/source.rs                # Source, from_file, find_project,
                                      # resolve_source (moved verbatim from
                                      # pipeline.rs:23–100)

espforge-hardware/  (new lib crate,   # DECIDED in 1a (done): separate crate;
                      decided)          # absorbs bindings dispatch later
  mod.rs       # pub fn lower(checked) -> Result<Hardware, String>
               # pub fn buildable(checked) (moved from pipeline.rs:1048)
  gpio.rs      # gpio table + led/button lowering
  i2c.rs       # i2c_peripheral_name + parse_khz(i2c) + bus/component lowering
  spi.rs       # spi lowering (spi2-only rule, mode, cs checks)
  uart.rs      # uart_peripheral_name, uart_attr_peripheral, framing lowering
  wifi.rs      # wifi_spec (moved from pipeline.rs:253–321)
  conflicts.rs # all "both use GPIO{n}" / peripheral-clash checks

espforge/src/commands/pipeline.rs     # ~150–200 lines left: Checked, Invalid,
                                      # validate(), unsupported_section(),
                                      # re-exports; delegates the rest
```

Why `espforge-model` for the structs: it already owns `Meta`, `Project`,
`Artifact`. `Hardware` is the same kind of domain model — everything
(`emit::context::build_context_with`, `bindings`, CLI) needs it, and none
of them should own it. `emit::context` keeps only the minijinja `Value`
mapping.

Why a separate hardware module/crate for lowering: it is the only place
that needs *all three* of `ParsedNode` (espf) + `Meta/Answers` (options) +
`Hardware` (model). Today that triangulation happens inside a CLI command;
it should be a library with pure functions (`&Checked -> Result<Hardware>`).

### Steps

1a. Decide crate vs module. DECIDED (done 2026-10-24): new
    `espforge-hardware` lib crate. Rationale: device drivers will grow
    and it can later absorb `espforge-bindings` dispatch; the public
    function shape (`&Checked -> Result<Hardware, String>`) is identical
    either way, so this stays reversible. Created
    `espforge-hardware/` (`Cargo.toml` + `src/lib.rs` with
    `pub use espforge_model::hardware::*`), registered it in the workspace
    `Cargo.toml`, and gave it the triangulation deps it will need in 1d:
    `espforge-model` + `espf` + `espforge-options`. Lowering itself still
    lives in `pipeline.rs` until 1d–1e.

1b. Move specs first (mechanical, no logic change) — DONE 2026-10-24,
    with the 1a crate decision folded in:
    - Copied `LedSpec`, `ButtonSpec`, `I2cSpec`, `SpiBusSpec`, `SpiSpec`,
      `UartSpec`, `WifiSpec`, `Hardware` verbatim from
      `espforge-emit/src/context.rs:46–147` to `espforge-model/src/hardware.rs`,
      plus `is_rust_ident` (from `pipeline.rs`) as the shared helper the
      Target promises next to the structs.
    - `pub mod hardware; pub use hardware::*;` from `espforge-model/src/lib.rs`.
    - In `espforge-emit/src/context.rs`, deleted the local definitions and
      replaced them with `pub use espforge_model::hardware::{…}`; in
      `espforge-emit/src/lib.rs`, re-export the specs from `espforge-model`
      (not via `context`) so both `espforge_emit::Hardware` and
      `espforge_emit::context::Hardware` keep working. In
      `espforge/src/commands/pipeline.rs`, replaced
      `use espforge_emit::{…Specs}` with
      `use espforge_model::hardware::{…, is_rust_ident}` and deleted the
      local `is_rust_ident`. (`build.rs`/`check.rs` never imported spec
      types directly, so no change was needed there.) Keep both
      `espforge-emit` re-exports until step 1f, which removes them.
    - `espforge-hardware` (from 1a) re-exports `espforge_model::hardware::*`
      so its future lowering modules and callers share one import path.
    - Verified with `cargo test -p espforge-model` (2 passed),
      `cargo test -p espforge pipeline` (42 passed), and
      `cargo test -p espforge-model -p espforge-emit -p espforge-hardware`.
      Note: 3 `espforge-emit` failures (`leds_are_reachable…`,
      `hyphenated_options…`, `jtag_console…`: `False` vs `false`, unknown
      `tojson` filter) are pre-existing on `v3` HEAD — reproduced via
      `git stash` — and unrelated to this move.

1c. Extract source discovery:
    - Move `Source`, `from_file`, `find_project`, `resolve_source`
      (`pipeline.rs:23–100`) to `espforge/src/source.rs` (new file, `pub`).
    - `pipeline.rs` re-exports them (`pub use crate::source::*`) so
      `check.rs`/`build.rs` keep compiling; then update `check.rs` and
      `build.rs` to import from `crate::source` directly and drop the
      re-export.
    - Move the `find_project` test (`pipeline.rs` tests module) with it.

1d. Extract hardware lowering per bus (do one bus at a time: wifi → i2c →
    uart → spi → gpio/led/button; each is independently reviewable):
    - Signature per module, e.g. `pub(crate) fn lower_i2c(nodes: &[ParsedNode],
      components: &[ParsedNode]) -> Result<(Vec<I2cSpec>, …), String>`.
      Start by moving code verbatim; do not refactor the internals yet.
    - Move helpers with their bus: `i2c_peripheral_name` → `i2c.rs`,
      `uart_*` → `uart.rs`, `wifi_spec` → `wifi.rs`, `parse_khz` → shared
      `util.rs` or duplicate the 20 lines per bus with a `tag` param (keep
      verbatim first).
    - Move the corresponding `#[test]` cases with each block (e.g.
      `a_uart_with_a_component_becomes_hardware` moves with `uart.rs`).
    - `hardware()` becomes a thin orchestrator: call each `lower_*`, then
      `conflicts::check_all(&…)?`, then assemble `Hardware`.

1e. Extract `conflicts.rs` last: move the five pin-ownership blocks
    (I2C-vs-GPIO, SPI bus pins vs everything, CS vs everything, UART-vs-all,
    UART-UART peripheral clash) into `pub fn check_all(…)`. Keep error
    strings byte-identical — several `check.rs`/`build.rs` tests assert on
    `"both use GPIO{n}"` and `"tx and rx"`.

1f. Thin out `pipeline.rs`: it keeps `Invalid`, `Checked`, `validate()`
    (~144–178), `unsupported_section()`, and a `hardware()` wrapper that
    calls into the new module. Delete the draft structs and helpers.
    Update `use espforge_emit::{…Specs}` → `use espforge_model::…` in
    `pipeline.rs` (already done in 1b), `build.rs`, `check.rs` (no spec-type
    imports found there yet — verify again here). Run `cargo test --workspace`.

### Acceptance

- `pipeline.rs` ≤ ~250 lines; `hardware/*` modules each ≤ ~250 lines.
- No `use espforge_emit::` for spec types anywhere in `espforge/`.
- `cargo test --workspace` green; error messages for bad pins/peripherals
  unchanged (grep tests for `both use GPIO`, `must be uart0`, `must be i2c0`,
  `must be spi2`).

### Risks / notes

- `Hardware` move touches `espforge-emit/src/context.rs` template field
  names — do not rename fields, only move the struct definitions.
- Keep `generate()` / `generate_with_hardware()` signatures in
  `espforge-emit/src/lib.rs:36–76` stable during this item; item 3 deals
  with emit’s API.

---

## 2. Unify the three parallel `$`-reference resolvers

### Problem

The same `$peripheral` link (e.g. `<led gpio="$gpio18">`) is resolved three
different ways, with three error styles:

1. `espf/src/context.rs` — `RuntimeContext::resolve_and_bind()` (~352 lines):
   generic graph over peripherals/components/devices. Errors are
   `EspfError` spans (rendered as snippets). This is what `validate()` calls
   (`pipeline.rs:144+` via `frontend::load` + `ctx.resolve_and_bind`).
2. `espforge-bindings/src/components/{gpio,i2c,spi,uart}_component.rs` —
   typed wrappers (`GpioComponent::resolve`, `I2cComponent::resolve`, …),
   each re-parsing the `$` ref with `trim_start_matches('$')` + interner
   lookup. Errors are ad-hoc `String`s with a `❌ [tag]` prefix. `check.rs`
   uses `route_driver()` (which uses these) for `# devices`, but
   `pipeline::hardware()` does *not* use them for `# components`.
3. `pipeline.rs::hardware()` (~323–1040) — hand-rolled third copy:
   `n.get_attr("gpio").trim_start_matches('$')`, linear `.iter().find()`
   over local `Vec`s, `String` errors (`"<led …> refers to …"`). It ignores
   `RuntimeContext`’s handles entirely, so a ref can pass the generic graph
   and fail here with a different message (or vice versa).

Also `frontend::load()` (`espforge/src/commands/frontend.rs:22–48`) +
`pipeline::validate()` + `pipeline::buildable()` split “is this valid?”
across three layers, so `check` has to remember to call all of them (see
`check.rs::execute`: `validate` → driver routing → `buildable` warning).

### Target

One resolution path: `RuntimeContext` is the index, `espforge-bindings`
wrappers are the only typed readers, hardware lowering consumes wrappers.

```text
espf/src/context.rs          # stays the raw graph: interner + handles +
                            # resolve_and_bind (unchanged API)

espforge-bindings/src/      # becomes the SOLE consumer of handles for
  components/*.rs            # bus components; add led/button/gpio-peripheral
                            # wrappers if missing so hardware/ never touches
                            # get_attr("$…") directly
  resolve.rs (new)           # pub fn component(node, ctx) -> Result<Component>
                            # (already exists as resolve_component — keep it,
                            #  extend to led/button)

espforge-hardware/* (from item 1)
  gpio.rs, i2c.rs, …         # call bindings wrappers instead of
                            # trim_start_matches('$') + Vec::find
```

Error policy: located problems (bad/missing ref, wrong parent type) are
`EspffError` spans; only flag/IO problems stay `String`. `hardware::lower`
returns `Result<Hardware, Vec<EspfError>>` (or `Invalid`) instead of
`Result<_, String>` so `check`/`build` render snippets uniformly.

### Steps

2a. Inventory the three copies (read-only, ~30 min):
    - List every `trim_start_matches('$')` in `pipeline.rs` and
      `espforge-bindings/src/components/*.rs`.
    - Note which `hardware()` errors already have span info available
      (the `ParsedNode` + `TagAttribute.key_offset/val_offset` carry it —
      see `espf/src/context.rs:27–52`) but discard it via `plain()` /
      `format!`.

2b. Extend bindings to cover what `hardware()` needs:
    - `gpio_component.rs`, `i2c_component.rs`, `spi_component.rs`,
      `uart_component.rs` already exist — audit their `REQUIRED_ATTRS` and
      `resolve()` against what `hardware()` validates (pin numeric,
      `frequency`, `mode`, `baud_rate`/`parity`/`stop_bits`, `cs`).
      Add missing checks to the wrappers, not to `hardware/`.
    - Add thin wrappers for `<led>` / `<button>` (they are components over
      `<gpio>` the same way `i2c_component` is over `<i2c>`). Reuse
      `GpioComponent`’s handle logic.
    - Keep `resolve_component()` / `TAGS` in `components/mod.rs` as the
      single dispatch; add the two new tags.

2c. Rewrite one bus in `hardware/` onto wrappers (start with I2C — smallest):
    - Replace the manual `bus_ref.trim_start_matches…` + `i2c_buses.find…`
      block with `I2cComponent::resolve(node, &ctx)`.
    - Convert wrapper `String` errors to `EspfError::at_node(node, …)` at the
      call site (or change wrappers to return `EspfError` directly — prefer
      the latter; update `resolve_component` callers in `check.rs` too).
    - Keep the user-visible message text identical where tests assert on it;
      only the rendering (snippet vs `❌ …`) changes. Update those tests
      deliberately, one assertion at a time.

2d. Repeat 2c for SPI, UART, GPIO/LED/button, then delete the local draft
    structs (`I2cBusDraft` etc.) and the manual `$` parsing. `hardware/`
    should contain no `trim_start_matches('$')` when done
    (enforce with `rg "trim_start_matches" espforge-hardware/`).

2e. Collapse the validation entry points:
    - Make `pipeline::validate()` the single function `check` and `build`
      call: `frontend::load` → flag overrides → `resolve` → `ctx` →
      `hardware::lower` (for the buildable subset). `check.rs` keeps its
      driver-routing loop, but `buildable()` becomes
      `hardware::lower(...).map(|_| ())` — delete the separate
      `unsupported_section`/`buildable` special cases or keep them as thin
      wrappers with a deprecation comment.
    - Change `hardware::lower`’s error type to `Vec<EspfError>` (via
      `Invalid::Spans`) so `Invalid::report` renders snippets for wiring
      errors too. `build.rs`’s `pipeline::hardware(&checked).map_err(user)?`
      becomes a snippet report like the `validate` failure path just above it.

### Acceptance

- `rg "trim_start_matches" espforge/src/ espforge-hardware/` returns only
  bindings internals (zero hits outside `espforge-bindings/`).
- `hardware/` imports from `espforge_bindings::components`, not from raw
  attr strings, for every parent-ref lookup.
- Wiring errors (unknown ref, double-use pin, wrong direction) render as
  snippets with file/line, not bare `❌ …` lines.
- `cargo test -p espforge -p espforge-bindings -p espf` green; any changed
  message assertions are explicitly called out in the PR.

### Risks / notes

- This changes user-visible error rendering — intentional, but keep message
  *text* stable and snapshot-test one example per bus before/after.
- `sections/espforge.rs::parse_meta` (231 lines, fixed schema for
  `# espforge`) is *not* part of this item — leave it; it already returns
  spans correctly.
- Do not touch `RuntimeContext`’s handle/interner design here; just stop
  bypassing it.

---

## 3. Untangle `render` naming and the pure-generation vs filesystem boundary

### Problem

Three things called “render”, two files literally named `render.rs`, and
pure vs IO code mixed in both crates:

- `espforge/src/render.rs` (555 lines) — pure help-text views for
  `espforge espf` (`render_root`, `render_section`, `render_man`, …).
- `espforge-emit/src/render.rs` (1,032 lines) — minijinja environment setup
  + `metadata.toml` parsing (`parse_metadata`, `EmitEntry`, `Metadata`,
  `glob_match`) + project planning (`plan`, `Planned`) + overlay layering
  (`render_project_with_overlay`, `OverlayFile`) + `readme()`. One file,
  three jobs.
- `espforge-emit/src/lib.rs:55–150` — pure `generate_with_hardware()` sits
  next to impure `write()`, `write_assets()`, `write_all()` (drift detection
  via `manifest::emit_manifest`/`parse_manifest`). Callers must read the
  whole file to know which functions touch disk.
- `espforge/src/commands/spec.rs` (183 lines: `effective_document`,
  `blank_document`, `render`) + `espf/src/writer.rs` (255 lines:
  `write_document`) + `build.rs` helpers (`overlay_files`, `app_logic`,
  `find_app`, `runtime_source`, `cargo_build`, `collect_dir`) spread
  “files in → files out” logic across CLI, espf, and emit.

A newcomer cannot answer “where do generated files come from?” without
opening five files in three crates.

### Target

```text
espforge/src/help_view.rs   # RENAME of espforge/src/render.rs (no logic change)
                            # update mod render; in main.rs

espforge-emit/src/
  template_env.rs  # environment(), render_template(), all_files(),
                   # collect_files()  (from render.rs:40–105)
  metadata.rs      # EmitEntry, Metadata, parse_own, check_glob, glob_match,
                   # parse_metadata(), metadata()  (from render.rs:107–210)
  project.rs       # Planned, plan(), render_project(),
                   # render_project_with_overlay(), OverlayFile, Rendered,
                   # readme()  (from render.rs:211–end)
  lib.rs           # generate*() (pure) + write*() (IO) with doc-comments
                   # marking the boundary; re-export template_env/metadata/
                   # project for compat during migration
  context.rs       # keeps build_context_with(); spec structs move to model
                   # (done in item 1) — otherwise unchanged here

espforge/src/source.rs      # (from item 1) ALSO gains overlay_files(),
                            # collect_dir(), find_app(), app_logic() moved
                            # out of build.rs:101–200
  spec.rs stays, but re-export via source.rs or keep as
                            # commands::spec (it already pairs with writer.rs)
```

Rule going forward: `*-emit` pure functions take/return in-memory
`Artifact`/`Asset`/`Value` and never touch `std::fs`; only `write*` and
`build.rs::execute` touch disk/processes (`cargo_build`, `std::fs::write`
of `app.rs`).

### Steps

3a. Rename first (zero logic change, one PR if you like):
    - `git mv espforge/src/render.rs espforge/src/help_view.rs`; update
      `mod render` → `mod help_view` and all `crate::render::` imports
      (in `commands/espf.rs` + tests).
    - In `espforge-emit`, split `render.rs` into the three files above by
      moving code verbatim with `pub(crate)` visibility adjusted. Keep
      `render.rs` as a one-line `pub use` shim during the move, delete it
      once `lib.rs` + callers import from the new paths.
    - Run `cargo test -p espforge -p espforge-emit`.

3b. Draw the IO boundary in `espforge-emit/src/lib.rs`:
    - Add module docs: `generate*` = pure (no fs), `write*` = impure.
    - Move `read_prev_manifest()` (currently private in `lib.rs:78+`) next
      to `manifest.rs` as `pub(crate) fn read_prev_manifest(out_dir)`.
    - No signature changes in this step.

3c. Move file-collection out of `build.rs`:
    - Move `OVERLAY_FILES`, `collect_dir`, `overlay_files`, `find_app`,
      `app_logic`, `runtime_source` from `build.rs:101–230` into
      `source.rs` (from item 1) as `pub fn overlay_files(src: &Source)`,
      `pub fn app_logic(src: &Source)`, `pub fn runtime_source(…)`.
    - `build.rs::execute` keeps only: `resolve_source` → `validate` →
      `write_spec?` → `generate_with_hardware` → `write_all` → copy `app.rs`
      → `cargo_build?`. It should read as the 10-line pipeline it is.
    - Move the `builds_the_*` integration tests’ helpers with the code;
      keep `cargo_build` (process-spawning) in `build.rs`.

3d. Document the query path once at the top of `help_view.rs` and
    `commands/spec.rs`: `Catalog (options) → help_view (strings) →
    espf.rs (print/page)` and `ParsedSections + Meta/Answers →
    spec::effective_document → writer::write_document → String`.

### Acceptance

- No file named `render.rs` in two crates simultaneously; `rg "mod render"`
  returns zero hits.
- `espforge-emit/src/render.rs` gone; `template_env.rs` / `metadata.rs` /
  `project.rs` each ≤ ~350 lines.
- `build.rs` has no `collect_dir`/`overlay_files`/`app_logic`/`find_app`;
  `source.rs` owns all “read the user’s directory” logic.
- Pure functions (`generate*`, `build_context_with`, `plan`,
  `render_project*`, `spec::*`, `writer::*`) import no `std::fs` /
  `std::process` (enforce by inspection: `rg "std::fs|Command::new"`
  in those files returns nothing).

### Risks / notes

- Renames churn imports — do step 3a alone and land it before 3b/3c.
- `wokwi.rs` (`chip_section`, 56 lines) and `manifest.rs` (83 lines) stay
  as-is; they’re already well-scoped.
- `espforge/src/sections/` currently holds only `espforge.rs` (`parse_meta`
  fixed schema). Leave it; if per-section schemas ever grow, that directory
  is where they go — note this in the PR so nobody invents a second place.

---

## Suggested order and PRs

1. PR A (item 1b–1c): move specs to model + extract `source.rs`. Small,
   mechanical, unblocks the rest.
2. PR B (item 1d–1e): per-bus hardware modules + conflicts.
3. PR C (item 2): bindings unification (one bus at a time; I2C first).
4. PR D (item 3a): renames + emit split.
5. PR E (item 3b–3c): IO boundary + `build.rs` slim-down.

Each PR: `cargo test --workspace` + a manual `espforge check` and
`espforge gen` on one LED example and one I2C example to confirm identical
output (`diff -r` the `build/` dirs before/after).

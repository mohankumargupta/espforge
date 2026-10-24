# Phase 2: typed IR + diagnostics (extreme.md §3.3, §3.4, §7)

**Status:** complete at increment 6 (2026-10-24). Increments 1–6 landed. `check` and
`build` lower into a `Board` and emit its `Hardware` projection; the `$` graph is the
owned `espf::context::Refs`; and the old middle — `RuntimeContext`, the typed wrappers,
the drafts/`attach_*` steps, `conflicts.rs` and the dual-run test — is deleted. `check` is
now the same single act as `build` (a hardware error exits 1 and never prints
`✓ … is valid`), every diagnostic carries a stable code, and `check --format json`
returns the diagnostics plus the board. The panic audit is
`espforge/tests/panic_audit.rs`: 129-peripheral and 129-component specs, huge numbers,
malformed attributes, bad references, truncated/duplicated files and a 1 MiB line all
end in a diagnostic, a clean refusal or a board. §5's metrics are measured at close and
§7 has the gate evidence. D7's four examples remain the only known corpus gap and stay
a separate reviewed decision. Phase 1 is complete (`phase1.md`): the schema is the
single source of truth and both halves are generated.

This file is the phase's plan and progress log. §2 lists what must be decided before the
first increment, §3 the increments and their gates, §5 the baseline this phase is measured
against. As work lands, update §3's status column and add evidence to §7.

---

## 1. Where Phase 2 started

The pipeline today is still the chain of text transformations extreme.md §2.2 describes,
with Phase 1's generated tables feeding the same wrappers:

```
.espf ──► espf::parser ──► frontend ──► options ──► RuntimeContext ──► hardware::lower ──► Hardware
          (winnow)        (structure)   (generated)  ($ graph, 128 slots)  (drafts + conflicts)   (7 Vecs)
                                                                                                   │
                       generated project ◄── minijinja ◄── Value map ◄──────────────────────────┘
```

Everything extreme.md §3.3 and §3.4 delete is still present and load-bearing:

| Representation | Where it lives now | Lines |
|---|---|---|
| `RuntimeContext`, `Handle`, `ActivePeripheral`, 128-slot `Vec<Option<_>>` tables | `espf/src/context.rs` | 352 |
| Typed wrappers with `resolve(node, ctx) -> Result<Self, String>` | `espforge-bindings/src/components/{gpio,i2c,spi,uart,led,button}_component.rs` | 560 |
| Schema-generated `Attrs` (Phase 1; this survives) | `espforge-bindings/src/components/generated_elements.rs` + `attrs.rs` | 1,308 |
| `*Draft` structs and `attach_*` / `lower_*` mutation | `espforge-hardware/src/peripherals/*.rs`, `components/*.rs` | ~1.4k |
| Post-hoc ownership pass | `espforge-hardware/src/conflicts.rs` | 406 |
| Seven parallel `Vec`s the emitter reads | `espforge-model/src/hardware.rs` | 120 |
| One-span `EspfError` with a title and a label | `espf/src/diag.rs` | 306 |
| `Invalid::{Spans, Message}` and `pipeline::buildable` | `espforge/src/commands/pipeline.rs` | 189 |

Verified at the phase start (and re-checked before increment 1 landed):

- `grep -rn 'struct Board\|struct ResourceGraph\|struct ClaimedPin'` finds only
  `extreme.md`; `Diagnostic`/`DiagCode` now live in `espforge-diagnostics`.
- `Cargo.toml` still lists the pre-refactor members; there is no `espforge-core`,
  `espf-syntax` or `espforge-cli`.
- `cargo test --workspace` was green at **294 passed, 1 ignored, 0 failed**; it is
  **329 passed, 1 ignored** after increment 4's production flip.
- The `check` half-state was real at the phase start: `espforge/src/commands/check.rs`
  reported `pipeline::buildable` errors as snippets and then printed
  `✓ <file> is valid` and returned 0. That is the `buildable()` split extreme.md §3.3
  said would collapse; increment 5 removed it.
- Phase 1 made this phase cheaper in three specific places: attributes already arrive as
  typed values with spans (no parsing in the IR), `<spi_component cs>` is already a
  `pin_or_ref`, and a write-only SPI bus is already `Option<u8>`.

## 2. Decisions to settle before increment 1

These are proposals, not choices made yet. Each one is expensive to change after its
increment lands, so settle it while the diff is small.

**D1 — Where do the IR and diagnostics live?**
extreme.md puts both in `espforge-core`, which is the Phase 4 crate split. Creating that
crate now would force the syntax/options move early. Proposal, taken: add a leaf
`espforge-diagnostics` crate now (it is below every other crate, so it cannot cycle), and
put `Board`/`ResourceGraph` in `espforge-model`, which stays dependency-free: a claim
returns structured `Conflict` data, and the lowering
that knows the element turns it into a `Diagnostic`. Phase 4 then merges `model` +
`hardware` into `core` without moving the code again.

**D2 — What is the diagnostic code scheme?**
Stable codes are a public contract (extreme.md §8.7). Proposal: `E01xx` syntax,
`E02xx` `# espforge` core/meta, `E03xx` options, `E04xx` `$` graph, `E05xx` peripherals,
`E06xx` components, `E07xx` ownership/conflicts, `E08xx` driver routing; `W0xxx` warnings.
Assign in one table in `espforge-diagnostics` so two stages cannot pick the same number.
This must be settled before increment 1, because JSON codes ship with increment 5 and
renumbering afterwards is a breaking change.

**D3 — Does anything from the old wording appear in human output?**
extreme.md's Phase 2 gate says every existing human diagnostic keeps its message and
primary span. Proposal: the human renderer stays byte-identical (codes are data, not
prefixes) and `--format json` exposes `code`; codes enter human output only if we decide
to break that gate deliberately. Corollary: `espf::diag` becomes a re-export of the new
crate for the duration of the phase, so call sites can migrate without a big-bang rename.

**D4 — How far does the IR go? (Appendix E Q4)**
Proposal: `Board` models the board, not the runtime construction order. Buses and
components keep source order because the emitter and every existing message depend on it.
No `Vec<Construct>` pass list is needed to satisfy any Phase 2 gate; if a backend wants an
explicit list later it can derive it deterministically from the board. `Hardware` survives
the phase as a projection (`Board::hardware()`) so the minijinja emitter and the golden
trees stay untouched; deleting it belongs to Phase 3's AST backend, not here.

**D5 — One diagnostic path, or two during migration?**
Proposal: `Diagnostic`/`Diagnostics` replace `EspfError` and `Invalid` outright in
increment 5, not aliased indefinitely. `Invalid::Message` (flag problems with no span)
moves into a `Diagnostic` with no primary span, or into a CLI-level error that never
enters the diagnostic list — decided when the first call site moves. The gate is that the
CLI has exactly one renderer and one exit-code mapping at the end.

**D6 — Conflict precedence is the sharp edge.**
Today `conflicts::check_all` runs after all lowering, so an invalid component later in
the file wins over an earlier pin conflict. Claim-on-insert reports the conflict when the
second claim happens, which can reorder errors. Proposal: keep the corpus green first by
claiming in today's order (peripherals in spec order, components in spec order, chip
selects after the buses) and treating any precedence change found by the corpus as a
reviewed diff, not a silent reorder. Do not weaken the corpus to make this pass.

A related discovery from increment 2: today's *wording* orders the two owners by which
`conflicts.rs` loop catches them, not by ownership (`i2c-vs-gpio` blames the gpio,
`uart-vs-i2c` blames the i2c, a chip select on its own bus blames the device). That order
cannot be derived from the claims, so `Conflict` carries both sides and the lowering keeps
the exact sentence and primary span. See increment 2 for what that changed in the gate.

**D7 — The four examples that cannot generate.**
`04.Communication/i2c_scanner` (a `<i2c_component>` with no `address`),
`i2c_example`, `spi_example_async` and `uart_example_async` (no `# services` section),
plus `examples/example.espf.txt` at the repo root. Fixing them is a separate,
reviewed corpus change: either the specs are stale, or `address` must be optional for a
bus-only component. Decide before the increment 5 gate claims "Board JSON covers every
example": today it can only mean every example that validates.

**D8 — Serialization dependency.**
The workspace has `serde_json` only in `espforge-emit` and no `serde` derive. `Board` JSON
and `--format json` need one or the other. Settled for increment 1 the cheap way:
`espforge-diagnostics` depends on `serde_json` only and builds a `Value` from the model, so
no derive feature was added and the rendered field set is explicit in one function. The
`Board` serializer can decide on `serde` derive when it lands; revisiting this for the IR
is the only open part.

## 3. Increments

Each increment ends with `cargo test --workspace` green and the contract suite unchanged
unless the increment explicitly says otherwise.

| # | Increment | Status |
|---|---|---|
| 1 | `espforge-diagnostics`: one diagnostic type, codes, human + JSON renderers | **landed 2026-10-24** |
| 2 | `Board`, `ResourceGraph`, `ClaimedPin`: the data model and claim rules | **landed 2026-10-24** |
| 3 | Lowering onto the IR, one element family at a time, with projection equality | **landed (dual-run) 2026-10-24** |
| 4 | Delete `RuntimeContext`, wrappers, drafts, `conflicts.rs` | **landed 2026-10-24** |
| 5 | One pipeline path: `check` builds a `Board`, `--format json`, baseline defects | **landed 2026-10-24** |
| 6 | Close: panic audit, metrics, gate evidence | **landed 2026-10-24** |

### Increment 1 — the diagnostics crate

**Landed.** `espforge-diagnostics` now holds `Span`, `Diagnostic`, `DiagCode`,
`Severity`, `Label`, `Edit`, `Fix`, `Diagnostics`, `to_human` and `to_json`.
`EspfError` keeps its constructor shape (`at_node`, `at_attr`, `at_key`, `at_span`) and
became a thin producer (`to_diagnostic`) of `Diagnostic`; `espf::diag::render_errors` now
delegates to `to_human`, so the existing constructors and snapshots compile unchanged. The renderer kept the
plain-vs-styled terminal decision, `term_width(100)` and the one-group-per-error layout,
and gained multi-line labels (the primary line first, one group, secondary annotations as
context) for the conflicts increment 5 will produce.

Two deliberate migration states:

- `Diagnostic::code` is `Option<DiagCode>`; `EspfError` conversion leaves it `None` and
  `to_json` emits `null`. Increment 5 assigns a code per message and drops the `Option`.
  The human renderer ignores codes, so no existing output moved.
- The JSON field order is alphabetical (serde_json's default map), pinned by
  `the_pretty_shape_is_pinned`. That is a test detail, not a contract, but it makes the
  snapshot honest.

The point was to move the container, not the wording. No message text changed, and the
two-label/help/fix capabilities are exercised only by new unit tests until increment 5 has
a stage that can honestly produce them.

Gate: the 24 diagnostics snapshots and 31 CLI transcripts are byte-identical; the
`espf::diag` tests keep passing; one new test renders a two-label diagnostic with help and
a fix to both human and JSON. See §7 for the run.

### Increment 2 — the IR data model

**Landed.** `espforge-model/src/board.rs` now holds `Ident { raw, span }`, `Pin`, `BusId`,
`PinUse`, `Owner`/`OwnerKind`, `Claim`, `ClaimedPin`, `Conflict`, `PeripheralClash`,
`ResourceGraph`, the three `Bus` shapes, `Framing`, the five component structs, `Component`,
`WifiService`/`Service` and `Board` (extreme.md Appendix C). A claim records the owning
element's id and node span plus the pin use (input/output/bidirectional/unspecified), so
the led/button clash is a graph rule rather than a second pass. `ClaimedPin` has private
fields and no public constructor: `ResourceGraph::claim` is the only way to make one.

`claim` returns `Result<ClaimedPin, Conflict>`, not `Diagnostic`. `Conflict` carries the
kind (`Pin`, `Direction`, `SelfClash`), the pin and the `existing`/`incoming` claims with
both spans; a refused claim leaves the graph untouched. A separate
`claim_peripheral` covers the UART/I2C peripheral-name clash, and `add_bus`/`bus` give
components `BusId` handles in creation order.

**The wording gate changed, deliberately.** The plan said the graph tests would assert the
same title, label and primary span as the corpus. They assert the pin, both owners and both
spans instead, because today's blame order is loop-specific: `i2c-vs-gpio` blames the gpio,
`uart-vs-i2c` blames the i2c, a chip select on its own bus blames the device, and a chip
select on another bus blames the bus. No ownership rule derives that. Keeping the sentence
in the lowering that knows the element is the same arrangement Phase 1 used for attribute
wording, and it is what lets increment 3 delete `conflicts.rs` without touching a single
expected diagnostic. The six corpus conflict cases are covered as claim sequences by
`the_diagnostics_corpus_conflicts_are_expressed_as_claims`.

Gate: `espforge-model` green (9 new tests, 11 in the crate); `cargo test --workspace`
**316 passed, 1 ignored**; nothing user-visible changed; the corpus is untouched.

### Increment 3 — lowering writes the IR

**Landed (dual-run).** `espforge-hardware/src/board.rs` builds a `Board` from the parsed
sections: peripherals in source order, then components, then services, claiming every pin
through `ResourceGraph::claim` as it goes. Attribute and `$`-reference validation still
comes from the Phase 1 `Attrs` and the typed wrappers; increment 4 inlines the remaining
occupancy/slug checks and deletes the drafts. All five families are covered: gpio/led/
button, i2c + i2c_component (frequency parse unchanged), spi + spi_component (write-only
bus, `cs` pin-or-ref), uart + uart_component (peripheral names, framing), and wifi/devices.

`Board::hardware()` in `espforge-model` projects the IR back onto the emitter's seven
vectors, preserving today's order (components for leds/buttons/spis, buses for
i2cs/spi_buses/uarts) and the `parity`/`stop_bits` template tokens. That projection is the
comparison surface: Phase 3's AST backend replaces it.

`tests/board_dual_run.rs` (deleted in increment 4 along with the old path) ran
`lower_spans` and `board::build` over every structurally valid embedded example. Where the old path succeeds, the projections are exactly equal;
where it fails, both paths fail. That covers helloworld, blink, button, spi_example,
uart_example and the wifi example — the same specs behind the 16 golden cells. Failing
specs are only asserted to fail on both sides for now; increment 4 makes wording and
primary spans identical.

Two tightenings the claims found, both real wiring errors the old path accepted: `<i2c>`
with `sda == scl`, and `<spi>` with `sclk == mosi`. They are reported as `i2c wiring` /
`spi wiring` instead of emitting two roles on one pin. Neither appears in the embedded
corpus; increment 4 should keep the rejection and record it as an explicit diff.

Gate: 10 new `board` unit tests, 2 dual-run tests, 1 projection test; `cargo test
--workspace` **329 passed, 1 ignored, 0 failed**. Production still calls the old `lower`,
so the diagnostics corpus, transcripts, goldens and the compile gate are untouched. The
new path's conflict wording is legacy-shaped but not yet byte-identical for every pair;
increment 4 pins it against the corpus.

### Increment 4 — flip production, then delete the old middle

**Landed 2026-10-24.** `pipeline::hardware` calls
`espforge_hardware::board::build(..).map(|board| board.hardware())`, so `check` and `build`
construct a `Board` and claim every pin on the way. `board::build` resolves the `$` graph
through `espf::context::Refs`, then parses the generated `Attrs` directly for led, button,
i2c_component, spi_component and uart_component. The emitter still reads the projection.

**Deleted, not bypassed:**

- `RuntimeContext`, `Handle`, `IdInterner`, `ActivePeripheral`, `ActiveComponent`,
  `ResolvedDevice` and the 128-slot tables (`espf/src/context.rs` now holds only
  `ParsedNode`, `TagAttribute`, `Refs` and `PeripheralKind`).
- The six typed wrappers and `resolve_component` in `espforge-bindings`, plus their tests.
- The hardware `components/` modules and every `attach_*` step; the `*Draft` types were
  reduced to the `I2cPeripheral`/`SpiPeripheral`/`UartPeripheral` values `board::build`
  needs, with `finish_*` deleted.
- `conflicts.rs`, the old `lower`/`lower_sections`/`lower_spans` and the `buildable_*`
  helpers; `espf/build.rs` and its unused generated registry were dead and are gone too.
- `tests/board_dual_run.rs`: its reference (`lower_spans`) no longer exists. The 24-case
  diagnostics corpus, 31 CLI transcripts, 16 golden cells and the compile gate now carry
  the comparison.

**Driver routing without handles.** `route_driver` takes
`(tag_name, &ParsedNode)` and the registry still dispatches to each driver's `init`; the
drivers are tag placeholders (`status_led`, `ili9341`, `tmp102`) and return `Ok` now that
binding validity comes from `Refs` and devices are rejected by `build`. The corpus case
`device-unknown-driver` keeps its exact plain-text message and exit code.

**Graph errors keep their wording and order.** `Refs::resolve` reproduces the old checks —
peripheral/component/device missing ids, component controller links, unknown
peripheral/component references and i2c pin parsing — so 23 of the 24 diagnostics cases are
byte-identical. The one deliberate change remains `conflict-led-vs-button`: the old label
repeated the incoming id (`<led id="btn">`); the claim records both owners, so it reads
`<led id="red_led">`. That closes the "a conflict names the wrong owner" defect early;
`check` printing `✓ … is valid` after a hardware error is still increment 5's fix.

Gate: no definitions or uses of `RuntimeContext`, `Handle`, `*Draft`, `attach_*`,
`conflicts::check_all` or `resolve_component` remain (only historical prose in
`phase2.md`/`extreme.md` and `espforge-model`'s design note); `cargo test --workspace`
**291 passed, 1 ignored, 0 failed**; compile gate green (19.0 s); 23 unchanged corpus cases
plus the one recorded owner fix; goldens and transcripts unchanged; the `espforge espf` CLI
untouched.

### Increment 5 — one path, one renderer, machine output

**Landed 2026-10-24.** `check` now runs `validate` then `pipeline::board`, the same
`Board::build` `build` uses. There is no `buildable()` and no second act: a hardware
error is a failure, so `check` exits 1 and never prints `✓ ... is valid` for a spec that
cannot be generated. Driver routing still runs before lowering, so a device tag keeps its
`No compile-time driver engine found` message instead of the temporary "devices are not
buildable" refusal.

**One diagnostic model at the CLI.** `Invalid::Spans` became
`Invalid::Diagnostics(Diagnostics)`; `frontend::report` takes `Diagnostic`s and prints
through `to_human`. `Invalid::Message` survives only as the CLI-level flag error D5
explicitly allowed: it has no source location, keeps the historical one-line `❌ ...`,
and is turned into a coded diagnostic for JSON. The source-level `EspfError` builders
remain as producers, but every message resolves through one code table, so the CLI never
sees a second diagnostic shape.

**Stable codes.** `espforge-diagnostics/src/codes.rs` assigns one code per message:
`E01xx` syntax, `E02xx` core, `E03xx` options/values/flags, `E04xx` `$` graph, `E05xx`
peripherals, `E06xx` components, `E07xx` ownership/conflicts, `E08xx` services and driver
routing. `Diagnostic::code` is now a `DiagCode`, not an `Option`; `E0999` is the
"unclassified" sentinel and the JSON test fails if it ever reaches output. The human
renderer still ignores codes, so no message text moved.

**`check --format json`.** One document on stdout:

```json
{
  "file": "embedded:01.Basics/blink",
  "diagnostics": [ { "code": "E0702", "severity": "error", "message": "pin already used", "primary": { "file": "...", "span": [173, 177] }, "labels": [...], "help": null, "fixes": [] } ],
  "board": null,
  "notes": []
}
```

A successful check puts the board in `board`; a failed one leaves it `null`. The board
view is `espforge/src/commands/board_json.rs` (the CLI, so `espforge-model` stays
dependency-free): metadata, pin/peripheral claims with owner spans, buses, components and
services, all with source spans. `serde_json` is a CLI dependency; `espforge-model` did
not gain one.

Then, and only then, the baseline defects from `phase0-baseline.md` §5 that the new shape
fixes for free:

| Defect | Outcome in Phase 2 |
|---|---|
| `check` contradicts itself | **fixed**: `Board::build` is the single act; 13 corpus snapshots moved from `exit 0` + `✓ ... is valid` to `exit 1`, message and span unchanged |
| a wrapper message leaks into a snippet label | **fixed**: attributes come from `Attrs`; earlier errors are `Diagnostic`s |
| a conflict names the wrong owner | **fixed in increment 4**: the claim records both owners |
| silent duplicate id / unknown section | **decision** — schema/validation rule, not an IR bug |
| out-of-range pins (`sda="99"`) | **decision** — needs the chip's pin table; may be its own increment |
| examples that cannot generate | **D7**, separate reviewed change |
| embassy corpus app is blocking | Phase 0 concern, unchanged here |

Gate: diagnostics corpus and transcripts updated as explicit reviewed diffs (no message
lost, primary spans preserved); `check --format json` exercised for every corpus spec and
every embedded example; `Board` JSON pinned for `01.Basics/blink` and required for every
example that lowers. `conflict-led-vs-button` remains the one earlier wording fix.

### Increment 6 — close

Audit the panic paths the phase claims to remove: the 128-slot tables, the
`at_span(0..0)` fallback in `conflicts.rs`, `parse_u8_attr`'s `unreachable!`, and a
129-peripheral spec. Add the adversarial specs to the corpus if they are not there.
Re-measure §5's table, fill §7, and check the gate in §6 line by line.

**Landed 2026-10-24.** The named panic sites are gone with their modules: `rg` finds no
`RuntimeContext`, `*Draft`, `attach_*`, `conflicts::check_all`, `parse_u8_attr`,
`resolve_component`, `at_span(0..0)` or `unreachable!` in any `.rs` file (only
historical prose in this file, `extreme.md` and two design notes). The 128-slot table
cannot overflow because there is no table.

**Adversarial cases now pin it three ways.** Three corpus specs were added, so the
text snapshots and the JSON contract both cover them: `peripheral-unsupported`
(`<pwm>` is refused, not ignored), `gpio-empty-pin` (an empty attribute value) and
`peripheral-many` (129 `<gpio>` entries, valid — the corpus deliberately contains one
success case now). `tests/panic_audit.rs` generates the rest without snapshots: 129
peripherals, 129 components claiming 129 pins, numbers past `u8`/`u32`/`u64`, negative
and space-prefixed pins, missing/empty ids and attributes, bare/dotted/unknown `$`
references, empty/header-only/CRLF/duplicated-section files, and a 1 MiB line. Every
case must exit 0 or 1, write nothing to stderr in JSON mode, parse as one document,
keep every span inside the source, assign a non-sentinel code, and carry a board
exactly when there are no diagnostics. A separate case runs human mode and fails on
`panicked at` or a spurious `is valid`.

One real `expect` was removed on the way: `source::app_logic` now returns a user-facing
error when a `Source` has no directory, matching `overlay_files`, instead of panicking
on an impossible-looking state. The remaining `expect`s in production code are build
scripts, `serde_json::Value` serialization, a piped-stdin child handle and a
`split('.').split_first()`, none reachable from a spec; the audit is about behaviour,
not a blanket ban on `expect`.

Gate: 8 new panic-audit tests, 3 new corpus cases (27 total), all green; workspace
**307 passed, 1 ignored, 0 failed**; compile gate green (19.72 s); `check_json`
enumerates the three new specs and still passes; goldens and transcripts unchanged.
Metrics are in §5 and the gate walk is in §7.

## 4. What Phase 1 handed over that Phase 2 must not undo

- The generated `Attrs` and their coercion helpers are the attribute layer; the IR must
  consume them rather than re-parse nodes. Frequency is the one deliberate exception
  Phase 1 left (`kind="freq"` stays text); Phase 2 moves its parse and its two messages
  together.
- The element wrappers own only `$`-reference lookup, slug checks and wording. Those are
  exactly the pieces that move into `Board::build` and the diagnostics constructors.
- `espforge-options` and the `espf` CLI are stable inputs to this phase; `catalog`,
  `generated_catalog`, completions and man pages are out of scope.
- The golden projects, transcripts and diagnostics corpus are the contract. A change to
  any of them is a product decision recorded here, not a test refresh.

## 5. Baseline and close

Phase 0 and Phase 1 columns measured before Phase 2; the close column measured
2026-10-24 on the final tree.

| Metric | Phase 0 | Phase 1 close | Phase 2 close | Phase 2 target |
|---|---|---|---|---|
| `cargo test --workspace` | 288 passed | 294 passed, 1 ignored | **307 passed, 1 ignored, 0 failed** | green throughout |
| Rust lines (excl. `target/`, goldens) | 18,795 | 22,597 | **23,964** (baseline +1,367) | lower; Phase 1 added 1,843 generated lines |
| `#[test]` functions in source | 288 | 303 attributes (294 run) | **316 attributes (307 run)** | fewer, stronger; goldens subsume string fragments |
| Files printing `❌` | 18 (13 live) | 18 files, 59 occurrences | **10 files, 38 occurrences** | 1 renderer, CLI only |
| `process::exit` in the CLI | 4 | 4 | **4** (`doctor` 1, `examples` 3) | 0 (Phase 4) |
| Error conventions | 5 | `EspfError`, `Invalid`, `String`, `anyhow`, `u8` | **`Diagnostics` at the CLI**, plus `Invalid::Message` for a flag with no span | 1 (`Diagnostics`) + exit mapping |
| Panics reachable from a valid-ish spec | ≥ 2 | 128-slot tables, `at_span(0..0)`, `unreachable!` | **0** (`tests/panic_audit.rs`) | 0 |
| `check` machine output | none | none | **`--format json`** | `--format json` |
| Dead modules | 2 | `espf/build.rs` + `generated_registry.rs`, `espforge/src/sections/` (232) | `espf/build.rs` gone; `espforge/src/sections/` (232) still there | Phase 4 |

Two targets were not met, and the table says so rather than hiding it. The line count
**rose** by 1,367: deleting the old middle removed ~2,000 net lines, but increment 1's
diagnostics crate, the IR, the board lowering and its tests, the JSON serializer and the
two contract suites added more than that. Similarly, tests went from 294 to 307 run,
not down. The phase's size estimate in extreme.md §5.6 (3.6k → 2.2k for
graph/IR/lowering, +0.7k diagnostics) described the moved code, not the new contract
tests; both are reported here as measured. The `❌` count is 10 files, but only the CLI
files render it — 18 of the 38 occurrences are the historical `❌ [...]` text inside
`espforge-hardware/src/board.rs` labels, which the corpus pins byte-for-byte. Removing
that prefix is a wording change for Phase 3/4, not something to smuggle into a close-out.

## 6. The gate

extreme.md §7, restated as checks that can fail:

1. **Every existing human diagnostic keeps its message and primary span.** The 24
   original diagnostics corpus files and the 35 CLI transcripts are byte-identical,
   except where §3's increment 5 table says a defect is fixed — the wrong-owner label
   landed early, in increment 4, and 13 corpus cases now exit 1 with no `✓ ... is valid`
   after a hardware error. Increment 6 added three cases without changing those 24
   (27 total). Each such diff is reviewed and listed in §7.
2. **`Board` JSON is stable and covers every example** (that validates — see D7).
   `tests/check_json.rs` enumerates the corpus and every embedded example, requires a
   board exactly when diagnostics are empty, checks every code/span, and pins the
   `01.Basics/blink` shape.
3. **The panic cases are gone.** The 129-peripheral spec, a malformed pin, a missing
   attribute, a zero-length span and a `<pwm>` all produce diagnostics or a clean refusal,
   never a panic. `peripheral-many`, `gpio-empty-pin` and `peripheral-unsupported` pin
   the first, second and last in the corpus; `structure-missing-sections` already pins the
   zero-width span; `tests/panic_audit.rs` generates the rest, including 129 components
   and values too large for the destination integer.
4. **The old middle is deleted, not bypassed.** No `RuntimeContext`, `*Draft`,
   `attach_*`, `conflicts.rs`; no second validation pass in the CLI. (Landed in increment 4;
   `grep` for the definitions is empty.)
5. **Contracts unchanged.** `.espf` grammar, CLI words/exit codes, generated project
   layout and ownership semantics, runtime API, embedded examples.

Commands:

```bash
cargo test --workspace
cargo test -p espforge --test golden_projects -- --ignored   # compile gate
```

## 7. Progress log

| Date | Increment | Evidence |
|---|---|---|
| 2026-10-24 | — | Phase 2 not started. Phase 1 closed; baseline measured above. |
| 2026-10-24 | 1 | `espforge-diagnostics` added (new workspace member, `annotate-snippets` + `serde_json`); `espf::diag::render_errors` delegates to `to_human`; 13 new tests. Workspace **307 passed, 1 ignored, 0 failed**; diagnostics corpus (24 cases) and CLI transcripts (31 cases) byte-identical; compile gate green (`generated_projects_compile`, 23.1 s); touched files rustfmt-clean. Codes are `None` until increment 5. |
| 2026-10-24 | 2 | `espforge-model/src/board.rs`: `Board`, `ResourceGraph`, opaque `ClaimedPin`, `Conflict` data, `Bus`/`Component`/`Service`, 9 new tests including the six corpus conflicts. `espforge-model` stays dependency-free; wording for a conflict stays with lowering. Workspace **316 passed, 1 ignored, 0 failed**; no production path uses the IR yet, so the corpus, transcripts and goldens are untouched. |
| 2026-10-24 | 3 | `espforge-hardware/src/board.rs` lowers all five families into `Board` with claims; `Board::hardware()` projection; `tests/board_dual_run.rs` compares against the old path over every structurally valid embedded example (equal projections, or both fail). 10 board unit tests (including the legacy titles/spans for led/led, uart tx/rx, i2c bus reuse and peripheral clash), 2 dual-run tests, 1 projection test. Workspace **329 passed, 1 ignored, 0 failed**. CLI still on the old path, so corpus/transcripts/goldens unchanged. Two new wiring rejections found (`sda == scl`, `sclk == mosi`) recorded for increment 4. |
| 2026-10-24 | 4 (flip) | `pipeline::hardware` switched to `board::build(..).map(|b| b.hardware())`; `check`/`build` now construct a `Board`. Workspace **329 passed, 1 ignored, 0 failed**; compile gate green; goldens and transcripts unchanged; one reviewed corpus diff — `conflict-led-vs-button` now names `<led id="red_led">` instead of repeating the button's id. Deletion of the old middle (route_driver handles, graph errors, `RuntimeContext`, wrappers, drafts, `conflicts.rs`) remains. |
| 2026-10-24 | 4 (graph + wrappers) | `espf::context::Refs` replaces `RuntimeContext::resolve_and_bind` for the production `$` graph: owned ids/peripheral kinds, same titles/labels/spans, same order. `board::build` calls it, and parses the generated `Attrs` directly for all five component families (the wrappers' wording is copied, not imported). `pipeline::validate` uses `Refs`; `Checked` no longer carries a context. Workspace **329 passed, 1 ignored, 0 failed**; compile gate green (19.4 s); diagnostics corpus (24 cases), transcripts (31) and goldens unchanged. `check` still builds a `RuntimeContext` only for `route_driver`, which is the remaining deletion blocker. |
| 2026-10-24 | 4 (delete) | Deleted `RuntimeContext`/`Handle`/`IdInterner`/`Active*`/`ResolvedDevice`, the six wrappers + `resolve_component`, the hardware `components/` + `attach_*`, the `*Draft` types and `finish_*`, `conflicts.rs`, the old `lower*`/`buildable*` helpers, the dual-run test and the dead `espf/build.rs`. `route_driver` now takes `(tag, &ParsedNode)`; the three driver `init`s are tag placeholders because devices are not lowered. Four board tests added for the deleted units' key wording (input/output direction, bad i2c address, undefined cs ref). Workspace **291 passed, 1 ignored, 0 failed**; compile gate green (19.0 s); 23 corpus cases unchanged plus the recorded owner fix; transcripts and goldens unchanged. |
| 2026-10-24 | 5 | `check` runs `validate` then `pipeline::board`; a hardware error exits 1 and prints no `✓ ... is valid` (13 corpus snapshots moved, messages/spans unchanged). `Invalid::Spans` became `Diagnostics`; `Invalid::Message` stays the CLI-level flag error allowed by D5. `codes.rs` assigns one code per message (E0101–E0809, `E0999` sentinel) and `Diagnostic::code` is no longer optional. `check --format json` returns diagnostics + board; the serializer lives in `espforge/src/commands/board_json.rs`, so `espforge-model` is still dependency-free. `tests/check_json.rs` enumerates all 24 corpus specs and every embedded example, checks codes/spans/board-null rules, pins `blink`, and asserts a hardware error is never reported valid. Workspace **299 passed, 1 ignored, 0 failed**; compile gate green (18.6 s on the final run); transcripts and goldens unchanged. |
| 2026-10-24 | 6 | Panic audit: the named sites are gone (`rg` for `RuntimeContext`, `*Draft`, `attach_*`, `conflicts::check_all`, `parse_u8_attr`, `resolve_component`, `at_span(0..0)`, `unreachable!` finds only prose). `source::app_logic`'s `expect` became a user-facing error. Corpus grew to 27: `<pwm>` refusal, empty pin, and a valid 129-peripheral case. New `tests/panic_audit.rs` (8 tests) generates 129 peripherals/components, oversized integers, malformed/missing attributes, broken `$` refs, empty/header-only/CRLF/duplicate-section files and a 1 MiB line; it requires exit 0/1, one JSON document, empty stderr, in-range spans, non-sentinel codes and a board iff clean. Workspace **307 passed, 1 ignored, 0 failed**; compile gate green (`generated_projects_compile`, 19.72 s); goldens and transcripts unchanged. Close metrics in §5: lines 23,964 (+1,367), tests 307 run, `❌` 10 files/38 occurrences, `process::exit` 4, panic cases 0, machine output present. |

### Gate walk at close

1. **Messages and spans.** `cargo test -p espforge --test diagnostics` passes all 27
   snapshots; the 24 originals are unchanged from increment 5's reviewed state, and the
   three new cases are additions. The 35 CLI transcripts pass unchanged in the workspace
   run. The only moved wording remains the increment-4 wrong-owner label and increment 5's
   13 exit-code/`✓` changes, both listed above.
2. **Board JSON.** `tests/check_json.rs` enumerates all 27 corpus specs and every embedded
   example: a board exactly when diagnostics are empty, every diagnostic coded and every
   span in range, and the `blink` shape pinned. The D7 examples fail validation and
   therefore report no board, which is the documented rule, not a gap in the enumerator.
3. **Panics.** `tests/panic_audit.rs` (8 tests) and the three corpus cases above cover the
   129-peripheral spec, malformed and missing attributes, the zero-width span (already in
   `structure-missing-sections`), `<pwm>`, oversized integers, broken references, empty
   and duplicated structure, and a 1 MiB line. No case exits by signal, prints
   `panicked at`, or emits a sentinel code.
4. **Old middle deleted.** `rg` across `.rs` files finds no `RuntimeContext`, `*Draft`,
   `attach_*`, `conflicts::check_all`, `parse_u8_attr`, `resolve_component`,
   `at_span(0..0)` or `unreachable!`; the only hits are historical prose in this file,
   `extreme.md`, `espforge-model`'s design note and the audit test's own doc comment.
   The CLI has one snippet renderer (`frontend::report` -> `to_human`); `Invalid::Message`
   is the single no-span flag error D5 allowed.
5. **Contracts.** The grammar, CLI words and exit codes, generated layout, runtime API and
   embedded examples are untouched; goldens and transcripts pass unchanged, and the
   compile gate builds the generated projects.

## 8. Open questions carried in

- **Appendix E Q1** (formatting/drift contract) is a Phase 3 prerequisite, not needed
  here; D4's `Board::hardware()` projection is what keeps the question out of Phase 2.
- **Appendix E Q3** (where docs prose lives) stays answered by Phase 1: prose sits inline
  in `espf.schema.kdl`.
- **Appendix E Q4** is D4 above.
- **Appendix E Q5** (driver registry from the schema) stays open. Phase 2 changes
  `route_driver`'s input from `RuntimeContext` handles to the validated `ParsedNode` and
  leaves the generated registry itself unchanged; driver bodies are placeholders until
  devices are lowered.
- **D7** is the one gate caveat that survives close: `04.Communication/i2c_scanner`,
  `i2c_example`, `spi_example_async`, `uart_example_async` and
  `examples/example.espf.txt` still fail validation, so "every example" in §6.2 means
  every example that validates. Fixing the specs or making `address` optional for a
  bus-only component is a separate reviewed corpus change, not a Phase 2 patch.
- **Phase 1 leftovers:** `kind="freq"` parsing moves in increment 3; `range=` is consumed
  by codegen but not rendered in docs, which remains a docs-surface question.

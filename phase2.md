# Phase 2: typed IR + diagnostics (extreme.md §3.3, §3.4, §7)

**Status:** in progress. Increments 1–3 landed on 2026-10-24; increments 4–6 are not
started. Phase 1 is complete (`phase1.md`): the schema is the single source of truth and
both halves are generated. The diagnostics crate, the IR data model and a lowering path
that builds a `Board` all exist and are dual-run against the old path. The CLI still calls
the old `lower`: drafts, `conflicts.rs`, the wrappers and `RuntimeContext` are the
production path until increment 4 flips it (see §7).

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
  **329 passed, 1 ignored** after increments 1–3.
- The `check` half-state is real: `espforge/src/commands/check.rs` reports
  `pipeline::buildable` errors as snippets and then unconditionally prints
  `✓ <file> is valid` and returns 0. That is the `buildable()` split extreme.md §3.3 says
  collapses.
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
| 4 | Delete `RuntimeContext`, wrappers, drafts, `conflicts.rs` | not started |
| 5 | One pipeline path: `check` builds a `Board`, `--format json`, baseline defects | not started |
| 6 | Close: panic audit, metrics, gate evidence | not started |

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

`tests/board_dual_run.rs` runs `lower_spans` and `board::build` over every structurally
valid embedded example. Where the old path succeeds, the projections are exactly equal;
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

### Increment 4 — delete the old middle

With lowering on the IR, remove `RuntimeContext` and its 128-slot tables, the six
wrappers and `resolve_component`, every `*Draft` and `attach_*`/`lower_*` mutation step,
and `conflicts.rs`. `Hardware` stays only as `Board::hardware()` for the emitter.
`espf::context::{ParsedNode, TagAttribute}` stays: it is the parsed syntax, not the graph.
The Phase 1 coercions (`Attrs`, generated elements, `attrs.rs`) stay and feed lowering
directly.

Gate: `grep -rn 'RuntimeContext\|\*Draft\|attach_\|conflicts::check_all'` is empty outside
docs and `extreme.md`; `cargo test --workspace` green; all snapshots unchanged; the
`espforge espf` CLI (`help`/`examples`) untouched.

### Increment 5 — one path, one renderer, machine output

Collapse `validate`/`buildable` into one act. `Board::build` is the only constructor, so
"valid but unbuildable" stops existing: `check` exits 1 and prints no `✓ ... is valid`
after an error. Replace `Invalid::{Spans, Message}` and the remaining `String` errors with
`Diagnostics`; the CLI prints through the one renderer. Add `check --format json`
(diagnostics + board) with stable codes and spans.

Then, and only then, the baseline defects from `phase0-baseline.md` §5 that the new shape
fixes for free:

| Defect | Outcome in Phase 2 |
|---|---|
| `check` contradicts itself | fixed by `Board::build` as the single act |
| a wrapper message leaks into a snippet label | fixed: attributes come from `Attrs`, earlier errors are `Diagnostic`s |
| a conflict names the wrong owner | fixed: the claim records both owners |
| silent duplicate id / unknown section | **decision** — schema/validation rule, not an IR bug |
| out-of-range pins (`sda="99"`) | **decision** — needs the chip's pin table; may be its own increment |
| examples that cannot generate | **D7**, separate reviewed change |
| embassy corpus app is blocking | Phase 0 concern, unchanged here |

Gate: diagnostics corpus and transcripts updated as explicit reviewed diffs (no message
lost, primary spans preserved); `check --format json` snapshots for every corpus spec;
`Board` JSON stable enough to snapshot for every validating example.

### Increment 6 — close

Audit the panic paths the phase claims to remove: the 128-slot tables, the
`at_span(0..0)` fallback in `conflicts.rs`, `parse_u8_attr`'s `unreachable!`, and a
129-peripheral spec. Add the adversarial specs to the corpus if they are not there.
Re-measure §5's table, fill §7, and check the gate in §6 line by line.

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

## 5. Baseline at Phase 1 close

Measured 2026-10-24, before any Phase 2 code.

| Metric | Phase 0 | Now | Phase 2 target |
|---|---|---|---|
| `cargo test --workspace` | 288 passed | **294 passed, 1 ignored** | green throughout |
| Rust lines (excl. `target/`, goldens) | 18,795 | 22,597 | lower; Phase 1 added 1,843 generated lines |
| `#[test]` functions in source | 288 | 303 attributes (294 run; 8 sit in the undeclared `sections/` module) | fewer, stronger; goldens subsume string fragments |
| Files printing `❌` | 18 (13 live) | 18 files, 59 occurrences | 1 renderer, CLI only |
| `process::exit` in the CLI | 4 | 4 (`doctor` 1, `examples` 3) | 0 (Phase 4) |
| Error conventions | 5 | `EspfError`, `Invalid`, `String`, `anyhow`, `u8` | 1 (`Diagnostics`) + exit mapping |
| Panics reachable from a valid-ish spec | ≥ 2 | 128-slot tables, `at_span(0..0)`, `unreachable!` | 0 |
| `check` machine output | none | none | `--format json` |
| Dead modules | 2 | `espf/build.rs` + `generated_registry.rs`, `espforge/src/sections/` (232) | Phase 4 |

The phase's own size, from extreme.md §5.6: graph/IR/lowering 3.6k → 2.2k, diagnostics
+0.7k. Those are estimates; §7 records what actually happened.

## 6. The gate

extreme.md §7, restated as checks that can fail:

1. **Every existing human diagnostic keeps its message and primary span.** The 24
   diagnostics corpus files and the 31 CLI transcripts are byte-identical, except where
   §3's increment 5 table says a defect is fixed — each such diff is reviewed and listed
   in §7.
2. **`Board` JSON is stable and covers every example** (that validates — see D7). A
   snapshot test enumerates the embedded examples and fails on a new field, not on
   formatting noise.
3. **The panic cases are gone.** The 129-peripheral spec, a malformed pin, a missing
   attribute, a zero-length span and a `<pwm>` all produce diagnostics or a clean refusal,
   never a panic.
4. **The old middle is deleted, not bypassed.** No `RuntimeContext`, `*Draft`,
   `attach_*`, `conflicts.rs`; no second validation pass in the CLI.
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

## 8. Open questions carried in

- **Appendix E Q1** (formatting/drift contract) is a Phase 3 prerequisite, not needed
  here; D4's `Board::hardware()` projection is what keeps the question out of Phase 2.
- **Appendix E Q3** (where docs prose lives) stays answered by Phase 1: prose sits inline
  in `espf.schema.kdl`.
- **Appendix E Q4** is D4 above.
- **Appendix E Q5** (driver registry from the schema) stays open. Phase 2 changes
  `route_driver`'s input from `RuntimeContext` handles to resolved devices; it does not
  change how the registry is generated.
- **Phase 1 leftovers:** `kind="freq"` parsing moves in increment 3; `range=` is consumed
  by codegen but not rendered in docs, which remains a docs-surface question.

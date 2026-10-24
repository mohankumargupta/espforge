# Phase 2: typed IR + diagnostics (extreme.md §3.3, §3.4, §7)

**Status:** in progress. Increment 1 (the diagnostics crate) landed on 2026-10-24;
increments 2–6 are not started. Phase 1 is complete (`phase1.md`): the schema is the single
source of truth and both halves are generated. The middle has not moved yet — there is
still no `Board`, no `ResourceGraph` and no `ClaimedPin`; `espforge-diagnostics` now exists
with the diagnostic type and both renderers, but the pipeline still produces `EspfError`
and `Invalid` (see §7).

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
  **307 passed, 1 ignored** after increment 1.
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
crate now would force the syntax/options move early. Proposal: add a leaf
`espforge-diagnostics` crate now (it is below every other crate, so it cannot cycle), and
put `Board`/`ResourceGraph` in `espforge-model`, which already owns the value types and has
no dependencies but `Diagnostic`. Phase 4 then merges `model` + `hardware` into `core`
without moving the code again.

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
| 2 | `Board`, `ResourceGraph`, `ClaimedPin`: the data model and claim rules | not started |
| 3 | Lowering onto the IR, one element family at a time, with projection equality | not started |
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

Add `Ident { raw, span }`, `Pin`, `BusId`, `ClaimedPin`, `Claim`, `Bus`, `Component` and
`Board` (extreme.md Appendix C), plus `ResourceGraph::claim` returning
`Result<ClaimedPin, Diagnostic>`. A claim records the owning element's id and span so a
conflict can point at both sides, and records the direction the pin is asked to carry
(input/output) so the led/button clash is a graph rule, not a second pass.

No pipeline wiring yet. Port `conflicts.rs`'s five corpus cases as graph tests and require
the same title, label and primary span — the first time the IR has to be as good as the
code it replaces.

Gate: new `espforge-model` unit tests are green; nothing user-visible changes; the corpus
is untouched.

### Increment 3 — lowering writes the IR

Rewrite `espforge-hardware` as lowering into a `Board`, family by family:

1. `<gpio>` + `<led>` + `<button>` (pin ownership and the input/output clash),
2. `<i2c>` + `<i2c_component>` (including the frequency parse moved from
   `util::parse_khz`, same wording),
3. `<spi>` + `<spi_component>` (write-only bus, `cs` pin-or-ref, chip-select exemption),
4. `<uart>` + `<uart_component>` (peripheral names, tx/rx, framing),
5. `<wifi>` and devices (`route_driver` reads the resolved board instead of handles).

After each family, dual-run old and new lowering over every embedded example, every
diagnostics-corpus spec and the golden specs, and compare `Board::hardware()` with the
old `Hardware`. Any difference is a bug in the new path, not a snapshot to refresh. The
projection must preserve today's order (buses first, then components) and today's first
error in file order (D6).

Gate: projection equality for the whole corpus, goldens byte-identical, compile gate
green, diagnostics corpus unchanged.

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

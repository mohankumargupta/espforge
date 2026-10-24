# Phase 3: AST codegen (extreme.md §3.5, §7)

**Status:** planned, not started (2026-10-24). Phase 2 is complete (`phase2.md`):
`check` and `build` lower into a `Board`, diagnostics carry stable codes, and
`check --format json` returns the board. The emitter still renders minijinja
templates from the `Hardware` projection; this phase replaces that with typed
backends and deletes the template tree.

This file is the plan and, once work starts, the progress log. §2 settles the
decisions that are expensive to change later (Appendix E Q1 is the first one),
§3 the increments and their gates, §5 the baseline the phase is measured
against, §6 the gate, §7 the evidence.

---

## 1. Where Phase 3 starts

Measured 2026-10-24 on the Phase 2 close tree (`cargo test --workspace`: 307
passed, 1 ignored).

| Piece | Today | Size |
|---|---|---|
| `espforge-emit` | minijinja context + template renderer + manifest/write | 2,374 lines, 54 tests |
| Project template | 13 files under `espforge-espgenerate-template/src/template` | 663 lines |
| `metadata.toml` | 12 conditional files, 7 ownership globs | 1 file |
| Golden projects | 16 cells, 13–15 files each (chip assets vary) | 216 files, 6,108 lines |
| Compile gate | RISC-V blocking examples + one embassy fixture | 6 cells |
| `Board` | the typed IR; `Board::hardware()` projects to the emitter | `espforge-model` |
| Overlays | example `diagram.json` rendered through the same minijinja context | 12 examples |

The chain to replace:

```
Board ──► Board::hardware() ──► minijinja Value ──► 10 rendered files
                                      │
                       metadata.toml (when/as/ownership)
```

`metadata.toml` decides which optional files exist (`ci`, editor configs,
`Embed.toml`, `hello_test.rs`), their output names and ownership; every other
template file is always emitted. Those optional entries are currently dead:
the files are listed in `metadata.toml` but absent from the template tree, so
`plan()` ignores them. `render_project_with_overlay` then replaces
`diagram.json`/`wokwi.toml` from an example and appends a `[[chip]]` section
when the example ships a Wokwi custom chip.

The contract this phase must keep: the generated project layout, ownership
semantics, CLI words and exit codes, the `.espf` grammar, the runtime API, and
the embedded examples. Byte-identical generated files are **not** part of that
contract; §2 D1 settles that explicitly.

## 2. Decisions to settle before increment 1

**D1 — Formatting contract (Appendix E Q1).**
Decision: canonical `prettyplease` output is acceptable; byte-identical output
is not a requirement. The point of the phase is that generated Rust is
parseable by construction, not that it preserves the template's hand-tuned
whitespace. Consequences, all deliberate:

- The four Rust files (`src/generated.rs`, `src/lib.rs`, `src/bin/main.rs`,
  `build.rs`) change shape once, when their backend flips.
- Golden projects are regenerated as reviewed formatting-only diffs, with the
  semantic comparator in D6 proving no token changed.
- Users' generated projects are unaffected at runtime; only their on-disk
  layout changes on the next build.

**D2 — Drift checksums do not need a migration.**
`write()` refuses only when the existing file matches neither the previous
manifest checksum nor the new content:

```rust
if checksum(&existing) != *prev_sum && existing != a.content { bail!("drift") }
```

An unedited file still matches `prev_sum`, so it is overwritten when the
emitter starts producing different bytes. The premise in extreme.md §8.1 —
that a formatting change makes every project look edited — is not true of this
implementation. Verified 2026-10-24 with a throwaway integration test: a file
written by an "old" manifest and then given different content by `write()` is
replaced, not refused. Therefore:

- No `template_version` migration, no whitespace-insensitive hashing, no
  "delete and regenerate" release note.
- `template_version` stays in the manifest as support metadata; it does not
  gate overwrites.
- A regression test is added: generate with an old manifest, produce a
  different body for the same path, and assert the unedited file is replaced;
  the existing edited-file refusal test stays.

If that reasoning is wrong for some deployed manifest format, the fallback is
to read `template_version` once and accept the old checksum for that build —
not to normalize whitespace, which would hide real edits.

**D3 — Overlay placeholders become a fixed token set.**
Example `diagram.json` files use exactly four placeholders: `{{ board }}`,
`{{ serialMonitor }}`, `{{ <component id>.pin }}` for leds and buttons (12 files,
4 distinct forms). Replace minijinja for overlays with a small typed resolver
that accepts only those forms and reports an unknown `{{ ... }}` as a user
error naming the supported tokens. This deletes the last template engine
instead of leaving a smaller one behind. If review wants arbitrary user
expressions preserved, the alternative is to keep minijinja for overlays only;
that postpones the deletion to Phase 4 and must be recorded as such.

**D4 — Where codegen lives now.**
Keep it in `espforge-emit` (`src/codegen/`), not a new crate: `espforge-model`
must stay dependency-free, and Phase 4's library split will move `emit` +
`model` into `espforge-core` anyway. Codegen may depend on `syn`, `quote`,
`prettyplease`, `serde` and `toml_edit`; the model may not. Per-component
emission is a trait or free functions in codegen over `Component`, not methods
added to the model.

**D5 — One source of truth per output family.**
Codegen consumes `Board` + `Meta` + `ResolvedOptions` + `RuntimeSource`
directly; the minijinja `Value` and `Board::hardware()` projection are deleted
once the last backend flips. Backends:

| Output | Backend | Comparison |
|---|---|---|
| `src/generated.rs`, `src/lib.rs`, `src/bin/main.rs`, `build.rs` | `syn`/`quote`, printed by `prettyplease` | parse both, unparse, compare |
| `Cargo.toml`, `.cargo/config.toml`, `rust-toolchain.toml`, `wokwi.toml` | `toml_edit` | parse both into `toml::Value` |
| `diagram.json`, `chip.json` | `serde` structs | `serde_json::Value` |
| `README.txt`, `esp-config.toml`, overlay assets | plain text/bytes | exact |

`FilePlan` in Rust replaces `metadata.toml`: a list of `(path, ownership,
condition)` built from the resolved options. Conditions are ordinary Rust
predicates (`opt.ci`, `opt.wokwi`, ...), not strings evaluated by a template
engine.

**D6 — Dual-run is semantic, not byte-exact.**
Before deleting a template, keep both renderers and compare their output:
parse old and new with `syn::parse_file`, unparse both with `prettyplease`, and
assert equality (same for TOML/JSON via parsed values). This proves the flip is
formatting-only without demanding identical bytes. Goldens may be regenerated
at each flip; the comparator is what makes that diff reviewable.

**D7 — Compile coverage and the Phase 2 D7 corpus gap.**
`04.Communication/i2c_scanner`, `i2c_example`, `spi_example_async`,
`uart_example_async` and `examples/example.espf.txt` still fail validation, so
"every example" cannot yet be true. Increment 0 resolves that corpus debt as a
separate reviewed change (make the specs complete, or make `address` optional
for a bus-only component — the spec owners decide which). The compile matrix
then grows to every generating example on `esp32c3` blocking, plus the embassy
fixture; Xtensa cells require espup in CI and stay explicitly listed rather
than silently skipped. Async examples can only compile with an async `app.rs`;
their existing app files are blocking, so the matrix records that pairing
instead of forcing an invalid runtime.

**D8 — Dependencies.**
Add to `espforge-emit`: `syn` (parsing + printing), `quote`, `prettyplease`,
`serde` with `derive`. Add `toml` as a dev-dependency for semantic comparison;
`toml_edit`, `serde_json` and `anyhow` already exist. Remove `minijinja` and
`include_dir` from `espforge-emit` in increment 4, and `include_dir` from
`espforge-espgenerate-template` (the crate keeps `VERSION`, the schema and the
overview prose).

**D9 — Comments in generated Rust.**
`quote!`/`proc-macro2` cannot carry ordinary `//` comments, and `prettyplease`
does not invent them. The generated files currently use comments for real
content: the file header, the per-bus/per-pin `SAFETY` justification for
`clone_unchecked`, and the "built first / hoisted" notes in `main.rs`. Dropping
them silently would be a quality regression. Decision: a narrow
`codegen/comments.rs` post-pass inserts the known comment lines at named
anchors after printing (e.g. the `let {bus}_bus =` line, each
`GPIO{pin}.clone_unchecked()` call, the `let components =` line), and a test
fails if an anchor is missing or ambiguous. The result is re-parsed before it
is returned, so a drifted anchor cannot produce invalid Rust. If that proves
too brittle, the fallback is to move each safety rationale into a doc comment
on a generated per-component helper, which `syn` can represent without a
post-pass; the comments are then API docs rather than inline notes.

## 3. Increments

Each increment ends with `cargo test --workspace` green. Golden changes are
allowed only when the increment says so and the semantic comparator proves
they are formatting-only.

| # | Increment | Status |
|---|---|---|
| 0 | Resolve the Phase 2 D7 examples, add them to goldens/compile matrix | not started |
| 1 | Seams: `FilePlan`, codegen dispatch, semantic comparator, dual-run | not started |
| 2 | Rust backend: `generated.rs`, `lib.rs`, `main.rs`, `build.rs` | not started |
| 3 | Data backends: Cargo/TOML/JSON/optional files | not started |
| 4 | Overlay tokens, delete templates and the `Hardware` projection | not started |
| 5 | Re-pin goldens, expand compile matrix, close metrics and gate | not started |

### Increment 0 — the corpus prerequisite

Fix the four failing specs and the root example, or record a reviewed decision
that an example is retired. Add every embedded example that now validates to
`MATRIX` (byte goldens) and to the compile matrix where its `app.rs` matches
the runtime. Gate: every embedded example passes `check`, every new golden is
reviewed, and `cargo test --workspace` is green. No codegen code yet.

### Increment 1 — seams and the oracle

Add `codegen/plan.rs` (`FilePlan` built from resolved options and ownership),
`codegen/mod.rs` with a `Backend` dispatch, and `codegen/equiv.rs`
(`same_rust`, `same_toml`, `same_json`). Wire a dual-run test that, for all 16
golden cells, renders the template tree and the codegen tree and compares them
semantically. No production flip.

Gate: output bytes unchanged; the comparator passes on all cells; a unit test
shows it rejects a changed token (so it is not vacuously true).

### Increment 2 — the Rust backend

Emit the four Rust files from `Board` with `quote!`, print with
`prettyplease`, and include the runtime-specific main (blocking vs embassy),
the wifi heap allocator, the SPI bus hoisting, the `SAFETY` comments and the
Xtensa `build.rs` branch. Flip `generate_with_hardware` to the new backend for
those four paths. Regenerate the affected goldens as formatting-only diffs
proven by `same_rust`.

Rust comments are handled per D9: insert the header and safety anchors, then
re-parse the printed file before returning it.

Gate: `same_rust` passes for all 16 cells (comments are compared as a separate
normalized stream, since `syn` drops them); every anchor is found exactly once;
workspace green; compile gate green for the 6 existing cells.

### Increment 3 — the data backends

Build `Cargo.toml` with `toml_edit` (including the conditional dependency
blocks and preserved comments where they carry information),
`.cargo/config.toml` and `rust-toolchain.toml` with the same typed builder,
`wokwi.toml` (including the typed `[[chip]]` section from `chip.json`), and
`diagram.json` with a `serde` `Diagram` struct. Drop the dead optional entries
(`ci`, editor configs, `Embed.toml`, `hello_test.rs`) from `FilePlan`; they are
not in the template tree, so nothing is emitted for them today. Flip the
remaining paths.

Gate: `same_toml`/`same_json` pass for every cell, including the Wokwi chip
example; workspace and compile gates green; no `metadata.toml` lookups remain.

### Increment 4 — overlays, deletion, projection removal

Replace overlay rendering with the fixed token resolver (`board`,
`serialMonitor`, component `.pin`), migrate the 12 example diagrams, and make
an unknown token a diagnostic. Delete `espforge-emit/src/{context.rs,
template_env.rs, metadata.rs}` and the template directory plus
`espforge-espgenerate-template/src/template`; drop `minijinja` and
`include_dir`. Change `build`/`pipeline` to pass `Board` to codegen, then delete
`Board::hardware()` and the `Hardware`/`*Spec` projection (the `*Spec` types
have no other consumer once codegen reads `Board`).

Gate: `rg` finds no `minijinja`, `include_dir`, `TEMPLATE_DIR`, `metadata.toml`
or `.template/` in source; all 12 example diagrams generate; workspace and
compile gates green.

### Increment 5 — re-pin and close

Regenerate all goldens with `UPDATE_GOLDENS=1` as one final canonical diff,
review it against the dual-run proof, and keep `same_rust` (or a saved
normalized copy) as the review artifact. Expand the compile matrix per D7.
Re-measure §5, fill §7, and walk §6.

Gate: the final goldens compile (6+ cells), generated Rust parses for every
cell, drift tests pass, and the close metrics are recorded.

## 4. What survives

- The generated project layout and ownership model (`Owned` / `SeedOnce` /
  `NotOwned`, drift refusal, README). Only the producer changes.
- `Board` as the single source of truth; codegen reads it directly.
- The CLI transcripts, diagnostics corpus, `.espf` grammar and runtime API.
- `espforge-espgenerate-template` for `VERSION`, `SCHEMA_KDL` and
  `OVERVIEW_TXT`; only its `src/template` tree and `include_dir` go.

## 5. Baseline and size estimate

| Metric | Phase 2 close | Phase 3 target |
|---|---|---|
| `cargo test --workspace` | 307 passed, 1 ignored | green throughout; +dual-run/unit tests |
| `espforge-emit` + template lines | 3,037 (2,374 + 663) | roughly flat; the win is invariants, not LOC |
| Golden cells | 16 | all generating examples × pinned chips/runtimes |
| Compile cells | 6 | every generating blocking example on esp32c3 + embassy fixture; Xtensa in CI |
| Template engine | minijinja | none (or overlays only, if D3's fallback is chosen) |
| `metadata.toml` | 12 conditions | Rust `FilePlan` |
| Output correctness | substring and no-`\n\n\n` assertions | parse by construction; semantic dual-run |

The phase is not expected to shrink the line count: `syn`/`quote` builders are
more verbose than templates. The measurable wins are that generated Rust always
parses, the `tojson`/`False` class of escaping bug disappears, and the runtime
contract is checked by compiling the output rather than by matching strings.

## 6. The gate

Restated from extreme.md §7 with the decisions above:

1. **Golden projects identical modulo formatting.** Every cell's Rust parses
   and unparses to the same program; TOML/JSON parse to the same values; plain
   files are byte-identical. The canonical bytes are pinned at close.
2. **`cargo check` green for the supported compile matrix.** Every generating
   blocking example on `esp32c3`, plus the embassy fixture; Xtensa cells run in
   a CI job with espup or are listed as an explicit, reviewed exclusion. No
   silent skips.
3. **The template stack is gone.** No minijinja/`include_dir`/`TEMPLATE_DIR`/
   `metadata.toml`/`.template/`; no `Hardware` projection; `FilePlan` is Rust.
4. **Drift behaves.** An unedited project is regenerated across the formatting
   change; an edited owned file is still refused; the manifest still records
   the template version.
5. **Contracts unchanged.** CLI transcripts, diagnostics corpus, project
   layout/ownership and the runtime API are untouched except for the reviewed
   formatting-only golden diff.

Commands:

```bash
cargo test --workspace
cargo test -p espforge --test golden_projects -- --ignored
```

## 7. Progress log

| Date | Increment | Evidence |
|---|---|---|
| 2026-10-24 | — | Phase 3 planned; Phase 2 closed. No production code yet. Baseline above. D2 checked with a throwaway `changed_output_overwrites_an_unedited_file` test (passed, file removed); the permanent regression test lands in increment 1. |

## 8. Open questions carried in

- **Appendix E Q1** is settled by D1: canonical formatting, semantic golden
  comparison, no byte contract. If review rejects `prettyplease`, the fallback
  is to run `rustfmt` on the printed output and compare normalized token
  streams, but the byte contract still does not return.
- **Appendix E Q4** (IR scope): Phase 2 chose board-only and kept source order;
  codegen consumes that order. An explicit construct list remains unnecessary
  unless a backend needs it.
- **Appendix E Q5** (driver registry): unchanged by this phase; devices are
  still not lowered.
- **Appendix E Q6** (migration UX): `template_version` is recorded but unused;
  a future `espforge migrate` can read it. D2 means this phase does not need
  it.

# EspForge: the extreme refactor

**Status:** design exploration, not a plan of record. No code changed.
**Companion:** `improve.md` covers three safe, incremental improvements. This document asks a
different question: *if we were willing to rewrite the middle of the tool, what should it become?*
It is deliberately higher-level: architecture, rationale, phases, costs — not function-by-function
edits.

---

## 0. The thesis in one sentence

> EspForge today is a chain of text transformations that keeps five copies of the same domain
> knowledge; the extreme refactor turns it into a small **compiler** — one schema, one typed IR,
> one diagnostics model, several backends — exposed as a **library**.

Everything below is a consequence of that sentence.

---

## 1. What "extreme" means here

| Not this | This |
|----------|------|
| Renaming modules, splitting files, adding traits | Replacing the *middle* of the stack: vocabulary → IR → backends |
| Keeping minijinja and tidying the context | Emitting Rust as AST; deleting the template project |
| Keeping the hand-written catalog and drift tests | Generating the catalog, docs, and validation from one schema |
| Keeping five error conventions and unifying the printers | A diagnostics crate with codes, severity, fix-its and JSON/SARIF |
| A one-way rewrite | A strangler migration with dual-run gates; the CLI and `.espf` grammar are frozen |

"Extreme" is allowed to delete about a quarter of the code, replace two crates, and add
capabilities (JSON diagnostics, `fix`, `migrate`, LSP, watch) that the current shape makes
expensive. It is *not* allowed to break the user-facing contracts in §6.

---

## 2. The current shape, stated plainly

### 2.1 Five representations of the same component

To answer "what is a `<spi_component>`?" you have to read all of these:

1. `espforge-espgenerate-template/src/elements.kdl` — the documented schema (248 lines).
2. `espforge-bindings/src/components/spi_component.rs` — a wrapper with `REQUIRED_ATTRS`, hand
   field parsing, and `Result<_, String>` errors.
3. `espforge-hardware/src/peripherals/spi.rs` + `components/spi_component.rs` — a `SpiBusDraft`,
   a `SpiDeviceDraft`, and an `attach_*` that re-checks what the wrapper checked.
4. `espforge-model/src/hardware.rs` — `SpiBusSpec`/`SpiSpec`, the shape the emitter wants.
5. `espforge-emit/src/context.rs` + `template/src/generated.rs` — a minijinja `Value` entry and a
   text template that turns it into Rust.

Plus a sixth, invisible one: the set of tests that assert the five agree
(`commands/espf.rs:220-260`, `bindings/src/components/mod.rs` "drift-checked" comments,
`elements.kdl` TODO notes about wrapper/catalog disagreement).

### 2.2 The shape of the pipeline

```
 .espf ──► espf::parser ──► frontend ──► options ──► RuntimeContext ──► hardware::lower ──► Hardware
            (winnow)        (structure)   (KDL schema)   ($ graph, 128 slots)  (drafts + conflicts)   (7 vecs)
                                                                                                       │
                       generated project ◄── minijinja ◄── Value map ◄──────────────────────────────┘
                       (text files)          (text!)      (stringly)
```

Three parsers of three formats (ESPF with winnow, `options.kdl` and `elements.kdl` with kdl), one
TOML config (`metadata.toml`), one JSON manifest, one stringly template context. Errors are
strings at three layers and a `Vec<EspfError>` at two others; six modules print `❌` (eleven call
sites), and none of them are library modules.

### 2.3 What that costs, concretely

| Symptom | Evidence in the repo |
|---------|----------------------|
| Adding one element touches 5–6 files and 3 drift tests | `<uart_component>` exists in wrapper, `TAGS`, `resolve_tag_required_attrs`, catalog, lowering, template context |
| The same rule is enforced twice, with different messages | I2C pins validated in `espf/src/context.rs:129` and `hardware/src/peripherals/i2c.rs:40`; ID slug checked in wrappers *and* in every `attach_*` |
| Template rot is found by unit tests asserting substrings | 3 tests currently fail: minijinja bool renders `False`, `tojson` filter is not registered (`project.rs:614`) |
| Generated code is never compiled by the test suite | `cargo_build` only runs in the `build` command; no in-repo CI; the justfile drives it by hand |
| A wiring error can lose its location | `conflicts.rs:27` falls back to `at_span(0..0, ..)` when a node lookup misses |
| A 129-peripheral file panics | fixed 128-slot tables, `espf/src/context.rs:99-100` |
| Dead scaffolding survives because nothing compiles it | `espforge/src/sections/` (232 lines), `espf/build.rs` + empty registry (61 lines) |
| `template_version` is recorded but never read | written in `manifest.rs`, dropped by `parse_manifest`; no migration path exists |
| The docs can disagree with the validator | `elements.kdl` TODO 11.11 notes wrapper/catalog mismatches |

None of this is a competence problem; it is what a text pipeline always looks like after a year.
Each layer was the cheapest place to add the next feature.

---

## 3. Target architecture

### 3.1 The picture

```
                      one schema (espf.schema.kdl)
                                │  build-time codegen
        ┌───────────────────────┼───────────────────────────┐
        ▼                       ▼                           ▼
  typed element structs    validation rules            catalog + docs
  (id, attrs, spans)       (parents, required)         (man pages, completions)
        │
        ▼
 .espf ──► espf-syntax ──► resolve ──► IR (Board) ──► validate ──► Diagnostics
          (lossless CST)   (names/$ refs)  (resources,      (codes, fix-its)
                                           components,
                                           services)
                                                │
              ┌─────────────────────────────────┼──────────────────────┐
              ▼                                 ▼                      ▼
        rust backend                     wokwi backend            docs / json
        (syn AST → rustfmt)              (serde → JSON)           (diagram, plan)
              │                                 │                      │
              └──────────────► FilePlan ◄───────┴──────────────────────┘
                                  │
                                  ▼
                        ownership manifest + drift rules
```

### 3.2 Move 1 — one schema is the source of truth

**Now:** `elements.kdl` (docs), `options.kdl` (options), Rust vocab tables, wrapper
`REQUIRED_ATTRS`, catalog loader (1410 lines), help renderer (558 lines), schema loader
(521 lines), plus hand-maintained agreement tests.

**After:** one machine-readable schema — start with the two KDL files merged, evolve to a single
`schema.kdl` — compiled by a small `build.rs` (or `xtask`) into:

- a Rust type per element (`struct SpiComponent { id: Ident, spi: Ref<Spi>, cs: PinRef, mode: u8 }`),
- attribute coercion (`Pin`, `Freq`, `Hex`, `Enum`, `Ref`) with spans,
- the parent/required rules the validator and wrappers share,
- the catalog data behind `espforge espf` and its man pages,
- the option schema consumed by the resolver,
- shell completions and LSP hover/completion data,
- the examples embedded in docs, validated by the same tests.

**Benefit:** adding `<pwm>` becomes one schema block plus one lowering function plus one emitter
function. The entire *class* of drift (and its tests) disappears because there is nothing to keep
in sync. The docs cannot lie about required attributes: they are generated from the rule the
validator executes.

**Risk to manage:** build-time codegen hurts IDE/rust-analyzer if overused. Keep the generated
file checked in and add `cargo xtask verify` as the gate; prefer plain structs over macros.

### 3.3 Move 2 — a typed IR replaces drafts, wrappers and parallel vectors

**Now:** `RuntimeContext` ($ graph), `*Draft` structs, `attach_*` mutation, seven parallel `Vec`s
in `Hardware`, and a `minijinja::Value` map. Resource ownership is checked after the fact by
`conflicts.rs` (403 lines).

**After:** one `Board`, built once, immutable afterwards.

```rust
// espforge-core/src/ir.rs (sketch)
pub struct Board {
    pub meta: Meta,
    pub resources: ResourceGraph,        // pins, i2c buses, spi buses, uarts
    pub components: Vec<Component>,      // typed, each holding ResourceId handles
    pub services: Vec<Service>,
}

pub enum Component {
    Led(Led),                    // { id: Ident, pin: ClaimedPin, active_low: bool }
    Button(Button),
    I2c(I2cComponent),           // { id, bus: BusId, address: u8 }
    Spi(SpiComponent),           // { id, bus: BusId, cs: ClaimedPin, mode: u8 }
    Uart(UartComponent),
    // adding Pwm is one variant + one impl
}

pub struct ResourceGraph {
    pins: BTreeMap<Pin, Claim>,  // claim on insert; conflict is an error with two spans
    buses: BTreeMap<BusId, Bus>,
}

impl Board {
    pub fn build(front: &Frontend) -> Result<Board, Diagnostics>;  // the only constructor
}
```

Consequences:

- **Validation and construction are the same act.** You cannot hold a `Board` that is invalid;
  conflicts are impossible to forget because the graph, not a later pass, enforces them.
- **No "unsupported hardware" half-states.** The `buildable()` vs `validate()` split
  (`pipeline.rs:104`) collapses: if `Board::build` succeeds, codegen can run.
- **Backends become total functions over a closed enum.** No string tags, no `unreachable!` for
  "unknown component", no `Hardware` ↔ `Value` mapping (30 sites in `context.rs`).
- **Machine output is free.** `espforge check --json` serializes `Board`; so does the LSP.

### 3.4 Move 3 — diagnostics become a product, not a side effect

**Now:** `espf::diag::EspfError` (title/label/span), `String` returns in wrappers, `Invalid` in the
CLI, `Failure`/`Stop`/`u8`/`anyhow` at the boundaries, and `❌` printed from six modules.

**After:** one type, one renderer family, stable codes.

```rust
pub struct Diagnostic {
    pub code: DiagCode,                  // E0104
    pub severity: Severity,              // Error | Warning | Note
    pub message: String,
    pub primary: Span,
    pub labels: Vec<(Span, String)>,
    pub help: Option<String>,
    pub fixes: Vec<Fix>,                 // machine-applicable edits
}
pub type Diagnostics = Vec<Diagnostic>;  // ordered, deduped, sorted by span
```

Renderers: `to_human` (annotate-snippets, exactly today's look), `to_json`, `to_sarif`. The CLI
prints; the library never does.

New capabilities this unlocks, all cheap once the model exists:

- `espforge check --format json` for CI and editor glue.
- `espforge fix` (apply `Fix` edits): add a missing `address`, rename a duplicated id, migrate a
  chip, rewrite a removed option.
- LSP diagnostics and quick-fixes with no extra parsing work.
- Stable snapshot tests: assert `code` + JSON, not English prose.
- Documentation can link to `E0104` instead of quoting a sentence.

This is the largest *product* change of the five moves, and the one that most improves
debuggability: today a user with a wrong pin gets a sentence; after, they get a numbered error with
a suggested edit.

### 3.5 Move 4 — codegen is AST + typed data, not text templates

**Now:** `espforge-emit` (2.3k lines) renders 17 embedded template files through minijinja with a
`Value` map; correctness is asserted by substring and "no `\n\n\n`" checks; the whole template
directory (`espforge-espgenerate-template/src/template`, 649 lines) must be kept in step with the
runtime API.

**After:** a `codegen` module with one backend per output family:

| Output | Backend | Guarantee |
|--------|---------|-----------|
| `src/generated.rs`, `src/lib.rs`, `src/bin/main.rs`, `build.rs` | `syn`/`quote` AST, printed with `prettyplease` | output is parseable Rust by construction; formatting is canonical |
| `Cargo.toml` | `toml_edit` (`DocumentMut`) | valid TOML, comment-preserving, merge-friendly |
| `diagram.json`, `wokwi.toml`, `chip.json` | `serde` structs (`Diagram`, `Wokwi`, `Chip`) | valid JSON/TOML by construction; no template escaping bugs |
| `.cargo/config.toml`, `rust-toolchain.toml` | small seed templates (or `toml_edit`) | still seed-once, but typed |

Each component owns its emission:

```rust
impl Component {
    fn emit_field(&self) -> TokenStream { .. }   // the `pub red_led: Led,` line
    fn emit_init(&self) -> TokenStream { .. }    // the constructor in `Components::new`
    fn emit_wokwi(&self, d: &mut Diagram) { .. } // the part + connections
}
```

Benefits over templates:

- **The `tojson`/`False` class of bug becomes impossible.** Values are typed; a string is a
  `LitStr`, a bool is a `LitBool`.
- **No whitespace archaeology.** `prettyplease`/`rustfmt` owns layout; the 13 assertions about
  `"\n\n\n"` and unrendered `{{` die.
- **Generated code can be compiled in tests.** Check in a golden crate (or generate into
  `target/` and `cargo check` it) for 3 chips × 2 runtimes. This is the invariant the repo lacks
  today and the one users feel most: `espforge build` is the first compile.
- **Composition instead of includes.** A shared preamble is a function, not a
  `.template/partials/*.rs` file that must be registered by path.
- **The runtime contract can be verified.** Either generate a tiny contract test from the same
  emitter (call every emitted constructor in a host-checkable shape) or have
  `espforge-runtime` export a capability table the backend validates. "Template calls a method the
  runtime no longer has" stops being a user-facing failure.

What survives: the *ownership model* (`Owned` / `SeedOnce` / `NotOwned`, drift refusal, README) —
that is good design and orthogonal to how bytes are produced. `metadata.toml`'s `emit`/`rename`
rules become a `FilePlan` in Rust.

### 3.6 Move 5 — library-first, with machine interfaces

**Now:** a single binary crate (4.5k lines) owning clap, prompts, paging, process spawning, the
help renderer, `doctor`, and the pipeline. Nothing else can use any of it; tests drive it through
`execute()` functions.

**After:**

```
espforge-core        schema, syntax, IR, validation, diagnostics, codegen   (pure, host)
espforge-drivers     device drivers + registry (from the schema)
espforge-runtime     target support (unchanged API)
espforge-cli         clap, cliclack, pager, cargo spawning  (~800 lines)
espforge-lsp         hover / completion / diagnostics over espforge-core
espforge             thin binary: `espforge-cli::main()`
```

This is what makes the other four moves pay off outside the binary:

- **CI integration**: `espforge check --format sarif/xml`.
- **Editors**: an LSP with the same diagnostics and completions as `espforge espf`, for free.
- **Testing**: integration tests call `core::build(request)` and assert on `Board`/`Diagnostics`;
  no temp dirs, no captured stderr, no process spawning.
- **A playground** becomes possible (core is pure; wasm-buildable).
- **Watch mode** is a small loop over the same entry point, with a file-watcher and a cache.

---

## 4. Worked example: adding `<pwm>`

**Today (nine touch points across five crates):**

1. `elements.kdl`: a `section="peripherals"` entry with attrs.
2. `espforge-model/src/hardware.rs`: `PwmSpec` + a field on `Hardware`.
3. `espforge-hardware/src/peripherals/pwm.rs` + a branch in `lower()`.
4. `espforge-bindings`: a wrapper if a component is involved (plus `TAGS`, `REQUIRED_ATTRS`,
   `resolve_component`).
5. `espforge-hardware/src/conflicts.rs`: extend every pyramid so PWM pins are protected.
6. `espforge-emit/src/context.rs`: a `pwm_value` closure and two `map.insert`s.
7. `espforge-espgenerate-template/src/template/src/generated.rs`: a loop.
8. `hardware/src/lib.rs` + `devices/mod.rs`: rewrite the "only <…>" messages.
9. `commands/espf.rs`: drift tests may need updating.

**After:**

1. `schema.kdl`: the `pwm` block (attrs, required, parent kind).
2. `ir.rs`: `Pwm(Pwm)` variant + fields.
3. `lower/pwm.rs`: `fn lower(node, &mut ResourceGraph) -> Result<Pwm>` — pins are claimed as part
   of construction, so conflicts are covered automatically.
4. `codegen/rust/pwm.rs`: `impl Component for Pwm { emit_field, emit_init }`.
5. `codegen/wokwi.rs`: nothing unless the diagram needs it.

Docs, `espforge espf` pages, completions, and required-attribute validation are generated. No
drift tests. The diff is ~120 lines in four files, and the "did I remember conflicts?" question
does not exist.

---

## 5. Benefits relative to the current repo

### 5.1 Comprehension

- **One place per concept.** Component = schema block + IR variant + lowering + emitter. Today it
  is five representations (§2.1).
- **The middle of the stack becomes readable prose.** `Board` is a data structure you can print
  (`Debug`), serialize, and inspect; the current pipeline state is spread over five structs and a
  `Value` map.
- **No archaeological comments.** "Moved verbatim from…", "step 2c pilot", "item 2e" (34
  occurrences) exist because the code is a migration log. A rewritten middle has no predecessors
  to reference.
- **Dead code disappears because it cannot compile.** 293 lines today survive only because nothing
  declares them.

### 5.2 Cost of change

| Change | Today | After |
|--------|-------|-------|
| Add a component | 5–6 files, 3 drift tests, 2 message strings | 4 files, no drift tests |
| Add an option | `options.kdl` + resolver tests + docs prose | schema block; resolver and docs generated |
| Change a diagnostic | edit a format string, grep fragment assertions | edit one `Diagnostic` constructor; tests assert code + snapshot |
| Change the generated project layout | template + `metadata.toml` + planner + tests | `FilePlan` + backend function |
| Support a new chip | `chips.rs`, `context.rs::wokwi_board`, template branches | one board/chip record in the schema-backed table |

### 5.3 Correctness

- **Invalid states become unrepresentable.** A `Board` exists only if it validated. No
  `buildable()` separate pass, no "valid but unbuildable" warning path.
- **Generated code is compiled by CI**, not by the user. Today the first compile of a template
  change is `espforge build` on a real project.
- **No stringly diagnostics.** Codes, spans, and fixes; a wrapper cannot invent its own
  `❌ [tag]` dialect.
- **No fixed-size tables, no `unreachable!` for user input.** The 129-peripheral panic and the
  `parse_u8_attr` panic path are structural, not bugs to patch.
- **Template/data escaping bugs vanish** because JSON is serialized and Rust is AST.

### 5.4 New capabilities (cheap after, expensive now)

| Capability | Why it is cheap after |
|------------|----------------------|
| `check --format json/sarif` | diagnostics are data |
| `espforge fix` | fix-its are part of diagnostics |
| `espforge migrate` | `template_version` becomes meaningful with a `FilePlan` diff |
| `espforge watch` | core entry point is a pure function of inputs |
| LSP / editor completions | catalog is generated from the schema |
| wasm playground | core has no I/O |

### 5.5 Testing

- **Fewer, stronger tests.** Drift tests (~10 assertions across `commands/espf.rs`,
  `catalog.rs`) are replaced by construction. Whitespace assertions (13) disappear.
- **Golden tests become possible** where they are impossible today: generated project bytes,
  rendered diagnostics, `Board` JSON.
- **Two test layers replace five**: unit tests inside core modules, plus end-to-end goldens
  (spec → project → `cargo check`). The 281 `#[test]` functions include a lot of string-fragment
  assertions that goldens subsume.

### 5.6 Size, honestly estimated

Order-of-magnitude accounting (not a promise):

| Area | Now | After | Δ |
|------|-----|-------|---|
| syntax (`parser`, `value`, `writer`, `diag`) | 1.7k | 2.0k | +0.3k |
| diagnostics (new crate: codes, renderers, fixes) | — | 0.7k | +0.7k |
| schema + generated vocabulary | 0.8k (2 KDL files + loader) | 0.7k schema, tables generated | −0.1k |
| options catalog / help / `# espforge` reader | 2.2k | 0.9k generic renderer | −1.3k |
| graph / IR / lowering (model + bindings + hardware) | 3.6k | 2.2k | −1.4k |
| emit + template directory | 3.0k | 1.9k (backends + seeds) | −1.1k |
| CLI + dead modules | 3.9k | 2.5k | −1.4k |
| runtime + examples | 2.0k | 2.0k | 0 |
| crate manifests + build scripts | 0.9k | 0.6k | −0.3k |
| **Total** | **18.1k** | **~13.5k** | **−4.6k (~−25%)** |

…while *adding* JSON diagnostics, fix-its, LSP, watch, and CI-compiled output. The numbers are
estimates; the direction is what matters.

---

## 6. What stays invariant (the contracts)

An extreme refactor is only acceptable because these do not move:

1. **The `.espf` surface syntax.** Existing files must parse and mean the same thing. Rewriting
   the parser is allowed; changing the grammar is not (new syntax may be additive).
2. **CLI compatibility.** Command names, flags, aliases, `help`/`examples` view words, exit codes
   (0 / 1 / 2 / 130), and the piped `command<TAB>description` format of `espforge espf`.
3. **Generated project layout and ownership semantics.** Same paths, same `Owned`/`SeedOnce`/
   `NotOwned` behaviour, same drift refusal, same README contract.
4. **The runtime API that `src/app.rs` uses.** `Context`, `Components`, `peripherals`, `logger`,
   `delay`, the component trait surface. Moving it is a separate project with its own migration.
5. **The embedded example corpus.** Same specs, same app logic, same expected output.

Everything else — crate layout, module names, template engine, error types, how docs are produced —
is fair game.

---

## 7. Migration: four phases, each with a gate

This is a strangler migration. The old pipeline keeps working until the new one proves itself.

**Phase 0 — freeze the contracts (≈2 days).**
Add the tests that do not exist today:

- transcripts for every CLI command (stdout/stderr/exit code),
- a diagnostics snapshot corpus (every message in a matrix of bad specs),
- golden generated projects for `01.Basics/{helloworld,blink}`, `04.Communication/i2c_scanner`,
  `02.Digital/button`, for blocking+embassy and esp32c3+esp32s3,
- a job that runs `cargo check` on those generated projects (this is the missing CI).

*Gate: the 3 known `espforge-emit` failures are fixed (preferred) or quarantined first, and the
new contract suite is green.*

**Phase 1 — schema-driven front (≈1.5 weeks).**
Introduce `schema.kdl` and the codegen for element types, required attributes, and catalog data.
Keep the existing validator behind a feature flag; run both and diff the catalog output.

*Gate: `espforge espf` output is byte-identical for the existing catalog; the drift tests can be
deleted because the new tables are generated from the same file.*

**Phase 2 — IR + diagnostics (≈2.5 weeks).**
Build `Board` and the diagnostics crate. Rewrite lowering over the IR. Delete `conflicts.rs`,
`*Draft`, the wrappers, and `RuntimeContext`. Render diagnostics through the new model behind
`--format human` (same output) and add `--format json`.

*Gate: every existing human diagnostic keeps its message and primary span (additive labels/help
are reviewed as explicit diffs); `Board` JSON is stable and covers every example; the panic cases
are gone.*

**Phase 3 — AST codegen (≈2 weeks).**
Write the Rust/JSON/TOML backends. Dual-run: generate with templates and with AST, diff the trees,
then flip. Keep templates until the golden projects match.

*Gate: all golden projects identical (modulo formatting, which the manifest can absorb — see
§8.1) and `cargo check` green for every example × chip × runtime.*

**Phase 4 — library split + new surfaces (≈1 week).**
Split `espforge` into `espforge-core`/`espforge-cli`/thin binary. Add `--format` machinery, then
`fix`/`watch`/LSP as follow-ons (each is a small project now).

*Gate: CLI transcripts unchanged; the CLI crate contains no validation logic.*

Total: roughly **6–8 weeks** for one experienced Rust developer, or a quarter with review and
buffer. The old code is deleted, not kept behind flags, at the end of each phase.

---

## 8. Costs, risks, and the sharp edges

### 8.1 Drift checksums will churn

The manifest stores a checksum per owned file (`manifest.rs`) and refuses to overwrite a file that
was edited. Any formatting change in emitted output (prettyplease vs the template's exact bytes)
makes every existing generated project look *edited*, so the next `build` refuses with "drift
detected".

Mitigations, pick one before Phase 3:

- keep `template_version` in the manifest and actually *read* it: on the first build after the
  upgrade, accept the old checksum once and rewrite the manifest (this finally gives
  `template_version` a purpose), or
- make the drift check formatting-insensitive (normalize whitespace before hashing), or
- accept a one-time "delete and regenerate" for generated projects, announced in the release.

This is the single most user-visible risk and it deserves an explicit decision, not a discovery.

### 8.2 AST codegen constrains output shape

`prettyplease` formats the whole file. Templates can produce hand-tuned layout that users have
grown used to. Decide: is canonical formatting acceptable? (Recommended: yes; it is the point.)

### 8.3 Build-time codegen vs rust-analyzer

Generated Rust from `build.rs` can confuse IDE navigation and cache invalidation. Check in the
generated file, gate it with `xtask verify` in CI, and keep tables as plain `const` data rather
than macros where possible.

### 8.4 Feature freeze and merge pain

Phase 2 rewrites the middle; concurrent feature work in `hardware`/`emit` will conflict. Budget a
short freeze or sequence the work behind the schema/first IR.

### 8.5 Loss of escape hatches

Templates can express odd one-offs (the Wokwi `[[chip]]` append, zig source seeding). Backends
must offer explicit hooks (`fn post_process(&mut FilePlan)`) or those cases get uglier, not
cleaner. Inventory them before Phase 3: `wokwi.rs`, `project.rs` overlay rules, and the seed-once
`chip/` handling are the known ones.

### 8.6 The "compiler" framing can be over-applied

Do not build a general-purpose IR with visitors, passes and a plugin ABI. There is exactly one
front-end and three backends. Resist traits for their own sake; a closed enum + `match` is the
right polymorphism here.

### 8.7 Diagnostics as a new dependency surface

Stable codes and JSON are a public contract once shipped. Number them deliberately, document the
ranges, and never reuse a code.

---

## 9. How we would know it worked

Measure before (Phase 0) and after:

| Metric | Today | Target |
|--------|-------|--------|
| Files touched to add one element | 5–6 | ≤ 4 |
| Drift-consistency tests | ~10 assertions | 0 |
| Error types / modules printing `❌` / `process::exit` sites | 5 / 6 / 4 | 1 / 1 / 0 |
| Generated project compiled in CI | no | every example × 3 chips × 2 runtimes |
| Panics reachable from a valid-ish spec | ≥ 2 | 0 |
| Lines in the workspace | 18.1k | ≤ 14k |
| `check` machine output | none | JSON + SARIF |
| Dead modules | 2 | 0 |

Qualitative: a new contributor can answer "how does `<spi_component>` become Rust?" by reading
four files in one order, and the answer is true for every other element too.

---

## 10. What we deliberately would *not* do

- **Not a new surface syntax.** The `.espf` grammar is the product; a YAML/TOML rewrite would break
  every project for no architectural gain.
- **Not a plugin system.** No dynamic loading, no scripting language for backends.
- **Not a general compiler framework.** No pass manager, no SSA, no visitor boilerplate.
- **Not a web service.** The pure core makes a playground possible; hosting is out of scope.
- **Not a runtime rewrite.** `espforge-runtime` stays; the refactor's job is to stop it drifting
  from codegen, not to replace it.
- **Not "keep both pipelines forever".** Dual-run is a migration tool, with a deletion date.

---

## Appendix A — file disposition

| Now | Fate |
|-----|------|
| `espf/src/{parser,value,writer}.rs` | keep; becomes `espf-syntax`; delete compat entry points |
| `espf/src/context.rs` | delete; `$` resolution moves to core IR |
| `espf/src/diag.rs` | grow into `espforge-diagnostics` (codes, fixes, renderers) |
| `espf/build.rs`, `espf/src/generated_registry.rs` | delete (dead today) |
| `espforge-model/src/{lib,hardware}.rs` | keep the value types; specs folded into the IR |
| new `espf.schema.kdl` | merge `elements.kdl` + vocabulary from wrappers; source of truth |
| `espforge-options/src/schema.rs` | replaced by generated option types |
| `espforge-options/src/catalog.rs` (1410) | replaced: loader for generated tables + generic renderer |
| `espforge-options/src/{resolve,section,chips}.rs` | keep logic; consume IR/schema |
| `espforge/src/help_view.rs` (558) | shrink to a generic renderer (~150) |
| `espforge-bindings/src/components/*` (719) | delete; typed element structs are generated |
| `espforge-bindings/src/drivers/*` | becomes `espforge-drivers`, registry generated from schema |
| `espforge-hardware/src/peripherals/*` | delete drafts; lowering writes the IR |
| `espforge-hardware/src/components/*` | delete; IR variants + `lower` impls |
| `espforge-hardware/src/conflicts.rs` (403) | delete; `ResourceGraph::claim` |
| `espforge-hardware/src/services/wifi.rs` | becomes an IR service + lowering |
| `espforge-emit/src/{context,template_env,metadata,project,wokwi}.rs` | delete; `codegen/{rust,wokwi,files}` |
| `espforge-emit/src/manifest.rs` | keep; add `template_version` migration |
| `espforge-espgenerate-template/src/template/**` (649) | delete except seed files (`.cargo`, toolchain) |
| `espforge-espgenerate-template/src/{elements,options}.kdl` | merge into the schema; `overview.txt` becomes generated or stays prose |
| `espforge/src/commands/*` | becomes `espforge-cli`; `pipeline`/`frontend`/`source` move to core |
| `espforge/src/sections/` (232) | delete (dead today) |
| `espforge-runtime` | keep; add a capability contract checked by codegen tests |

## Appendix B — crate graph

```
                 espf-syntax
                      │
                 espforge-core ──────► espforge-diagnostics
                 │        │
                 │        ├──────────► espforge-drivers
                 │        └──────────► espforge-runtime  (target-only)
                 │
        ┌────────┴─────────┐
   espforge-cli        espforge-lsp          espforge (bin)
```

Rule: dependencies point down; `core` has no I/O, no `clap`, no `cliclack`, no `std::process`, no
`println!`.

## Appendix C — the IR in more detail

```rust
pub type Span = std::ops::Range<usize>;
pub struct Ident { pub raw: String, pub span: Span }     // `red-led` + where it was written
pub struct Pin(pub u8);
pub struct BusId(pub usize);

pub struct Led   { pub id: Ident, pub pin: ClaimedPin, pub active_low: bool }
pub struct Button{ pub id: Ident, pub pin: ClaimedPin, pub pull_up: bool }
pub struct I2cComponent { pub id: Ident, pub bus: BusId, pub address: u8 }
pub struct SpiComponent { pub id: Ident, pub bus: BusId, pub cs: ClaimedPin, pub mode: u8 }
pub struct UartComponent{ pub id: Ident, pub port: BusId, pub baud: u32, pub framing: Framing }

pub enum Bus {
    I2c { id: Ident, sda: Pin, scl: Pin, khz: u32, used_by: Option<Ident> },
    Spi { id: Ident, sclk: Pin, mosi: Pin, miso: Pin, khz: u32, mode: u8 },
    Uart{ id: Ident, tx: Pin, rx: Pin, used_by: Option<Ident> },
}
```

`ClaimedPin` is only constructible by `ResourceGraph::claim`, which returns
`Result<ClaimedPin, Diagnostic>`; the owner label and span are recorded so conflicts report the
*first* owner, as today.

## Appendix D — a diagnostic, before and after

Today (roughly what `conflicts.rs` produces):

```
error: pin conflict
  --> blink.espf:6:3
   |
 6 |   <led id="red-led" gpio="$gpio18" />
   |   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ <led id="red_led"> and <led id="other"> both use GPIO18
```

After (richer human rendering, plus machine data):

```json
{
  "code": "E0104",
  "severity": "error",
  "message": "pin GPIO18 is already claimed",
  "primary": { "file": "blink.espf", "span": [68, 103] },
  "labels": [
    { "span": [68, 103], "text": "<led id=\"red-led\"> claims GPIO18 here" },
    { "span": [20, 48],  "text": "already claimed by <led id=\"other\">" }
  ],
  "help": "give one of the two leds a different <gpio>",
  "fixes": [
    { "title": "rename the second led", "edits": [ { "span": [76, 83], "text": "red-led-2" } ] }
  ]
}
```

The four fields that do not exist today — `code`, a second label, `help`, `fixes` — are what make
`fix`, LSP and CI annotations possible.

## Appendix E — open questions to settle before Phase 1

1. **Formatting contract.** Is canonical `prettyplease` output acceptable for generated files, or
   must bytes stay identical? (Decides the drift-migration strategy in §8.1.)
2. **Schema format and codegen mechanism.** One KDL file compiled by `build.rs`, or a TOML file
   read by an `xtask` that checks in Rust? (Recommendation: KDL → checked-in Rust, verified by
   `xtask verify`.)
3. **Where does the docs prose live?** `elements.kdl` currently mixes data and man-page text.
   Generated pages need a prose source; keep it in the schema with a clear "prose, not validated"
   marker.
4. **How far does the IR go?** Does it model the *runtime construction order* (buses before
   devices) or just the board? (Recommendation: board + explicit construct list; the order is a
   backend concern.)
5. **Driver registry.** Generate it from the schema (devices section) or keep `build.rs` scanning
   `drivers/*.rs`? (Recommendation: schema, so `check` and codegen agree without drift.)
6. **Migration UX.** `espforge migrate` in this refactor, or record `template_version` now and
   implement it later?

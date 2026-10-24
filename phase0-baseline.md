# Phase 0: the contract freeze (extreme.md §7)

**Status:** done for the layers listed below. No production code was refactored; this
phase only adds tests and fixes the red baseline they depend on.

The point of this phase is that every later gate ("no new failures", "identical output")
means something. Before it, `cargo test --workspace` was red and nothing compiled a
generated project.

---

## 1. What exists now

| Artifact | Where | What it pins |
|----------|-------|--------------|
| CLI transcripts (31 cases) | `espforge/tests/transcripts.rs`, `espforge/tests/transcripts/*.txt` | stdout, stderr and exit code of every non-interactive command: `new`, `check`, `gen`, `build` (error path), `examples *`, `espf *`, `version`, help, usage errors, `-C` |
| Diagnostics corpus (24 cases) | `espforge/tests/diagnostics.rs`, `espforge/tests/diagnostics/specs|expected/*` | the rendered snippet — title, message, primary span, source line — of each diagnostic layer: syntax, structure, `# espforge` core, options, `$` refs, peripheral validation, component validation, pin conflicts, driver routing |
| Golden projects (16 cells, 216 files) | `espforge/tests/golden_projects.rs`, `espforge/tests/golden/*` | the exact generated tree — `.espforge/manifest.json` checksums, seed-once files, binary `chip.wasm` included — for `helloworld`/`blink`/`button` × 2 runtimes × 2 chips, plus `spi_example`/`uart_example` × 2 chips (blocking) |
| Compile gate (6 cells) | same file, `-- --ignored` | `cargo check` on freshly generated projects — the invariant the repo lacked. 5 corpus examples (blocking) plus `tests/fixtures/embassy_app`, which supplies the async `app.rs` the corpus lacks |
| CI | `.github/workflows/ci.yml` | runs both; the compile job is the one that was missing |

Refreshing a snapshot is always an explicit, reviewed diff:

```bash
UPDATE_TRANSCRIPTS=1 cargo test -p espforge --test transcripts
UPDATE_DIAGNOSTICS=1 cargo test -p espforge --test diagnostics
UPDATE_GOLDENS=1     cargo test -p espforge --test golden_projects
```

---

## 2. Baseline fixes (they had to come first)

`cargo test --workspace` failed **four** tests, not the three `improve.md` knew about:

| Test | Cause | Fix |
|------|-------|-----|
| `context::tests::leds_are_reachable_by_list_and_by_name` | minijinja renders a bool as `False`; the test expected `false`. No template prints a bool (they use `{% if %}`), so the test was asserting a renderer quirk | assert the branch (`{% if %}`) instead of the spelling; behaviour unchanged |
| `context::tests::hyphenated_options_are_reachable_by_index` | same | same |
| `project::tests::jtag_console_and_description_reach_cargo_toml` | `unknown filter: tojson` — the template needs it, `minijinja` had no `json` feature: a real template bug, not a test bug | enable the `json` feature in `espforge-emit/Cargo.toml` |
| `espforge_examples::tests::examples_embed_and_parse` | asserted `categories[0].examples[0] == "helloworld"`, but examples are documented as sorted by name and `blink` sorts first — stale test | find `helloworld` in `01.Basics`; also pin the sorted-by-name order |

`cargo test --workspace` is green: **288 tests**.

---

## 3. The §9 metrics, measured before Phase 1

Source only: excludes `target/` and `espforge/tests/golden/` (which adds 216 generated
files on purpose).

| Metric | Today | Target (extreme.md §9) |
|--------|-------|------------------------|
| Workspace Rust lines | 18,795 | ≤ 14,000 |
| `#[test]` functions | 288 | fewer, stronger |
| Files printing `❌` | 18 (13 live, 5 in dead/`build.rs` scaffolding) | 1 |
| `process::exit` sites in the CLI | 4 (`doctor`, `examples` ×3) | 0 |
| Error conventions | 5 | 1 |
| Generated projects compiled in CI | **now yes**: 6 cells (every example × both runtimes, esp32c3) | every example × 3 chips × 2 runtimes |
| Panics reachable from a valid-ish spec | ≥ 2 | 0 |
| `check` machine output | none | JSON + SARIF |
| Dead modules | 2 (`espforge/src/sections/` 232 lines, `espf/build.rs` + empty registry) | 0 |

Not yet measurable, and left to the phase that creates the seam: "files touched to add
one element" (5–6 today), drift-consistency assertions (7 `drift` mentions across
`espforge`/`espforge-bindings`).

---

## 4. What the net does not cover yet

- **Xtensa compile cells.** `esp32s3` needs the `esp` toolchain from espup; the compile
  matrix is RISC-V (`esp32c3`) only. Those cells stay covered byte-wise by the goldens.
  Add them to `COMPILE_MATRIX` once CI installs espup.
- **No I2C golden, no embassy bus golden.** None of the I2C examples and neither
  `*_async` bus example generates today (see §5), so bus coverage is
  `spi_example`/`uart_example`, blocking only.
- **Embassy goldens are byte-only.** They pin what the generator emits, but the corpus
  `app.rs` files are blocking, so those trees would not compile. Embassy *compilation* is
  covered by the fixture instead.
- **Interactive paths.** `new` without `--yes` and bare `examples` are only pinned on
  their non-TTY refusal; the browser and prompt flows have no transcript.
- **`example.espf.txt` in `examples/`** (repo root fixture) is invalid: it has no
  `# services` section. It is a scratch fixture, not an embedded example.

---

## 5. Defects found while freezing (decide before Phase 2)

These are captured in the corpus exactly as they behave today. They are not Phase 0's job
to fix; they are the reasons Phase 2 exists, and each one needs a decision rather than a
silent change.

1. **`check` contradicts itself.** Problems found by the hardware stage render as
   `error:` snippets and then `✓ ... is valid` is printed with exit 0 — e.g.
   `conflict-i2c-vs-gpio`, `gpio-non-numeric-pin`, `i2c-bad-frequency`. Only
   validation-stage problems exit 1. This is the `buildable()` half-state extreme.md §3.3
   collapses.
2. **A wrapper message leaks into a snippet label.** `i2c-component-bad-address` renders
   `❌ [i2c_component] 'dev' invalid hex address '0xZZ'` *inside* the `error: invalid
   i2c_component` snippet.
3. **A conflict names the wrong owner.** `conflict-led-vs-button` says
   `<led id="btn"> and <button id="btn">`; the led is `red_led`. Both labels come from the
   button's own id.
4. **Silent acceptances**, pinned by name in `ref-duplicate-id-silently-accepted` and
   `structure-unknown-section`: a duplicated `<gpio id>` and an unknown `# widgets`
   section both pass `check` with no diagnostic at all.
5. **Out-of-range pins are not validated.** `<i2c sda="99" …>` on an `esp32c3` passes.
6. **Four embedded examples cannot generate:** `04.Communication/i2c_scanner`
   (`<i2c_component>` has no `address`, which the wrapper requires), and
   `i2c_example`, `spi_example_async`, `uart_example_async` (no `# services` section).
   Either the corpus is stale or `address` must be optional for a bus-only component —
   a schema question (§3.2), not a test question. `examples/example.espf.txt` at the
   repo root is missing the same section.
7. **Every corpus `app.rs` is blocking.** `--runtime embassy` on a corpus example
   generates a project that cannot compile (`main.rs` awaits `setup`/`forever`; the
   example defines the blocking signatures). The `*_async` counterparts that would fix
   this do not generate today (§5.6). The compile gate adds
   `tests/fixtures/embassy_app` (async app, LED spec) so embassy codegen is still
   compiled; the embassy goldens stay byte-only.

Note on gate design: the first compile run failed on `uart_example × embassy`, which is
exactly the kind of pairing that byte comparison happily freezes. That is the argument
for keeping the compile gate in CI rather than trusting the goldens alone.

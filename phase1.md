# Phase 1: one schema (extreme.md §3.2 / §7)

**Status:** increments 1, 1.1, 2, 3, 4, 5, 6 and 7 landed; Phase 1 is complete.
`espf.schema.kdl` is the single source of truth for the element vocabulary, the attribute
rules, the attribute coercion and the build options, and both halves are generated:
nothing parses a schema attribute by hand (`grep -r 'get_attr(' espforge-hardware/src`
is empty), `options.kdl` no longer exists, and neither the option table nor the catalog
is parsed at runtime. The loaders survive only as the oracle the generated tables are
tested against.

---

## 1. Decision: how the schema becomes code

Appendix E asked: one KDL file compiled by `build.rs`, or a TOML file read by an `xtask`
that checks in Rust?

**Chosen: KDL → `build.rs` → checked-in Rust**, which is what §3.2 recommends, with two
adjustments:

- No new `xtask` crate yet. `espforge-bindings/build.rs` already generated
  `src/drivers/generated_registry.rs` the same way, so the pattern and its failure modes
  (stale file, IDE confusion) were already known in this repo.
- CI enforces "checked in and current": the `test` job runs `git diff --exit-code` on the
  generated files after `cargo test`, so a schema edit that was not regenerated fails.

Why not `xtask`: it adds a crate and a second command to run; `build.rs` is already wired
into `cargo test`, which is what CI runs. Revisit if codegen grows enough that build times
or rust-analyzer suffer (§8.3).

## 2. What is generated

Plain structs and enums — no macros, so rust-analyzer and the debugger see ordinary code.
Four of the files come from `espf.schema.kdl`; the fifth,
`src/drivers/generated_registry.rs`, comes from the driver modules.

`src/components/generated_vocab.rs` — the `components` elements marked
`bus_component=#true`, in schema order, as `TAGS`.

`espforge-options/src/generated_options.rs` — the option half of the same file as a
`Schema` value: `required` and one `OptionDef` per `option`, spans included.
`schema::embedded_schema()` returns it, and `the_generated_table_matches_the_loader`
asserts it equals `schema::load(SCHEMA_KDL)` — so the loader stays the oracle for what the
schema means, and a transcriber mistake is a test failure rather than a silent divergence.
A bool option with no explicit `default` is transcribed as `Some(Bool(false))`, which is
the loader's rule, not a special case in the generator's head.

`espforge-options/src/generated_catalog.rs` — the element half behind `espforge espf`:
the `SectionDef`s and `ElementDef`s with their attributes, examples, `see_also` targets,
compatibility and byte spans. `catalog::embedded()` returns it, and
`the_generated_catalog_matches_the_loader` asserts it equals `catalog::load(SCHEMA_KDL)`
— the same transcriber-not-validator arrangement as the options half.

`src/components/generated_elements.rs` — one `pub mod <tag>` per element in the schema
(all twenty, not only the components), each with:

- `Attrs<'a>`, one field per declared attribute, typed by its `kind`:
  `Spanned<&'a str>` for `ident`/`str`/`freq`, `Spanned<u8>` for `pin`/`hex`,
  `Spanned<u32>` for `u32`, `Spanned<bool>` for `bool`, `Ref<'a>` for `ref`,
  `Vec<Spanned<&'a str>>` for `list`, and a generated enum for `choice`
  (`Direction`, `Parity`, `StopBits`, …). Optional attributes are `Option<…>`; a schema
  `default` makes the field non-optional and attributes the default to the element's span.
- `Attrs::from_node(node) -> Result<Self, Issue>` — presence, `default`, `range` and
  `choices` all come from the schema, in schema order (which is the order the wrappers
  already checked them in, so error precedence is unchanged).
- for `choice` attributes, an enum whose variants are the `choices` spellings, with
  `parse` / `as_str` / `CHOICES`. A numeric choice cannot be a variant, so `1` becomes
  `N1`; the doc comment on the variant shows the spelling.

The coercion itself is **not** generated: `src/components/attrs.rs` holds `Span`,
`Spanned<T>`, `Ref`, `Issue` and one helper per kind (`ident`, `text`, `flag`, `number`,
`number_in_or`, `pin`, `hex`, `reference`, `parsed`, `list`, …). It is the same code for
every element, and the generated file stays declaration-only.

Two deliberate boundaries:

- **`kind="freq"` stays text.** Unit parsing is peripheral-specific today (`400kHz`,
  `40MHz`, and i2c's "not supported yet" for the rest) and carries its own messages. A
  second parser in this layer would be a third vocabulary; Phase 2 moves the parse and the
  wording together.
- **The message is not generated.** A coercion returns `Issue::Missing`/`Issue::Invalid`;
  the wrapper that owns the element's wording turns it into the sentence users see. That
  is what kept every transcript and the diagnostics corpus byte-identical, and it leaves
  one place per element to change when diagnostics become codes (§3.4).

## 3. What was deleted (and the one thing that was not)

- `components::TAGS` and `resolve_tag_required_attrs` (hand-written), plus the six
  per-wrapper `REQUIRED_ATTRS` consts and drift test 9 (two hand-written copies of one
  fact).
- The six wrappers' attribute parsing: `get_attr` + `parse` + `trim_start_matches` per
  attribute, `GpioDirection::parse`, `Parity::parse`, `StopBits::parse`, and
  `i2c_component::parse_address` (plus its `parse_i2c_address` re-export). The wrappers
  now keep only the `$`-reference lookup, the slug check and their wording.
- The hand-written enums themselves: `GpioDirection`, `Parity` and `StopBits` are now
  re-exports of the generated enums, so adding a choice to the schema adds a variant.
- **In `espforge-hardware`:** every `get_attr` + `parse` for `pin`, `sda`/`scl`, `sclk`/
  `mosi`/`miso`, `tx`/`rx`, `tx`-style pins and `mode` on the peripherals, and `active_low`,
  `pull_up`, `baud_rate`, `parity`, `stop_bits` on the components. `<led>`, `<button>` and
  `<uart_component>` now read the wrapper's coerced fields; `<gpio>`, `<i2c>`, `<spi>` and
  `<uart>` read `Attrs` directly. `frequency` is still text (see §2) and still parsed by
  `util::parse_khz`, which is where its two messages live.

What hardware kept is what is genuinely its own: pin collisions and input/output clashes,
`baud_rate > 0`, the i2c/spi/uart peripheral-name rules, bus occupancy, and mapping a
schema `parity`/`stop_bits` choice onto the `esp_hal` variant spelling (an exhaustive
`match`, so a new schema choice cannot be silently defaulted).

## 4. Schema changes these increments needed

- `spi.mode` and `spi_component.mode` gained `default="0" range="0..=3"`: the wrapper's
  silent default and its refusal above 3 were rules living only in Rust. They are now
  where the man page reads them, which is the point of the whole exercise. The catalog
  renders the default (no transcript pins the `spi_component` page); `range` is consumed
  by the generator and not rendered yet.
- The header of `espf.schema.kdl` now documents the `attr` properties and both generated
  files.

One behaviour change falls out of this, and it is the schema finally being enforced:
`<gpio direction="...">` is a `choice`, so a value other than `output`/`input` is now an
error (`invalid gpio direction`) instead of being accepted and ignored. Nothing in the
examples, the golden projects or the diagnostics corpus used another value.

## 5. One file: the options merged in

`espforge-espgenerate-template/src/options.kdl` is gone. `required "chip"` and the
`options { … }` block now sit at the end of `espf.schema.kdl`, under a `// options`
heading, and the header states the rule that makes one file workable: **each reader skips
the other's nodes and still rejects anything unknown.**

- `catalog::load` ignores `required` and `options`; its unknown-node message now lists all
  four (`section`, `element`, `required`, `options`), so a typo is still caught.
- `schema::load` ignores `section` and `element`; same message.
- `OPTIONS_KDL` is deleted and `SCHEMA_KDL` is the only embedded text
- The template crate's `build.rs` no longer watches a second file.

That is the whole of extreme.md §3.2's "start with the two KDL files merged" step. The
other half — one schema compiled to code — followed in increment 5: `schema::load` is now
the build-time authority and the tests' oracle, not the runtime path. `embedded_schema()`
returns the generated table from a `LazyLock` (built once, on first use) and
`espforge-options/build.rs` is a transcriber only: it has no rules of its own, so the
schema's meaning still lives in one place.

Increment 6 finished the job for the element half: `catalog::load` had been run inside a
`LazyLock` that panicked on an invalid embedded file, so a schema mistake surfaced as a
panic in whatever command touched `espf` first. Both halves now share one shape — build
script writes the table, loader stays the oracle, equality test pins the two together —
so a bad schema or a bad transcriber fails `cargo test` instead, and the runtime path has
no parser and no panic left in it. The transcriber does have to restate the loader's
smaller rules (a bool option's implicit `Some(Bool(false))`, a missing attribute's
implicit `Status::Implemented`, first-vs-last duplicate handling); those are exactly what
the equality test checks, so a wrong guess there is a red test rather than a divergence.

Five generated files, one gate: CI's `git diff --exit-code` list names
`generated_options.rs` and `generated_catalog.rs` next to the three files
`espforge-bindings` writes.

## 6. Gate evidence

- `cargo test --workspace`: green, **289 passed, 1 ignored**.
- The compile gate (`cargo test -p espforge --test golden_projects -- --ignored`): green.
- `espf` CLI transcripts, the diagnostics corpus and the golden projects: unchanged.
  Nothing user-visible moved.
- Generated files are rustfmt-clean and stable: rebuilding produces no diff, and CI diffs
  all five after `cargo test`. The equality tests bite: hand-editing one span or one
  summary in `generated_catalog.rs` fails `the_generated_catalog_matches_the_loader` with
  "is stale: rebuild espforge-options".
- New tests: `the_generated_catalog_matches_the_loader` and
  `the_generated_table_matches_the_loader` (the two halves' equality checks), the coercion
  helpers (`attrs.rs`) and
  `schema_level_problems_keep_their_wording`, which pins the ten messages the generated
  layer sits behind — missing `gpio`/`address`/`cs`/`baud_rate`, a bad hex address, `mode`
  out of range, a bad `parity`, a bad `active_low`, and led/button's
  `needs gpio="$<gpio id>"`. Plus `a_zero_baud_rate_is_refused`, the one rule hardware
  still adds to a coerced value.

## 7. The two open drifts, decided (increment 7)

The drift the generated layer made visible was settled in favour of the schema both
times, so the schema keeps saying what the language means and code follows it.

**`<spi>` without `miso`: the schema was right.** A missing `miso` is now an error
nowhere; `SpiBusDraft`/`SpiBusSpec` carry `Option<u8>` and the generated call is
`SpiBus::build_write_only(..)` (new, both runtimes) instead of `SpiBus::build(..)`. The
bus ownership list for the clash checks holds two pins for such a bus, not a zero-filled
third — that was the one real hazard, pinned by
`an_absent_miso_does_not_become_gpio0_in_the_clash_checks`. The template branch only fires
for a write-only bus, so every golden project (all of which wire MISO) is byte-identical.

**`<spi_component cs>`: a new `pin_or_ref` kind.** `cs="9"`, `cs="gpio5"` and
`cs="$gpio5"` are one attribute kind now, declared in `espf.schema.kdl` and read as
`attrs::PinOrRef` (`Pin(Spanned<u8>)` or `Ref`). It needed a variant in `AttrKind` and a
`display()` spelling (`pin or ref`), a placeholder in `help_view`, and a `pin_or_ref`
helper in `attrs.rs` — the generated `Attrs` field and the wrapper's `cs` field follow
from it. The transcriber had no arm for the new kind, so the equality test failed
immediately (`kind: Str` vs `PinOrRef`) and the missing three lines showed up where they
belong: in `espforge-options/build.rs`, not in a user's build.

That change surfaced a second, pre-existing bug: **`cs="gpio5"` could never work.** The
wrapper requires the referenced id to exist, and the clash check then refused any
`<gpio>` sharing the CS pin — so the spelling the schema itself used in its example was
rejected as "both use GPIO5". `SpiDeviceDraft` now records which `<gpio>` the `cs` named,
and that one entry is exempt from the cs-vs-gpio loop; a `<led>`/`<button>` on the same
pin is still caught by its own loop. Tested both ways in
`a_cs_may_name_a_gpio_instead_of_a_number` and
`a_cs_named_gpio_still_clashes_with_a_component_on_that_pin`.

Gate evidence: `cargo test --workspace` green, **294 passed, 1 ignored**; the compile gate
green; golden trees unchanged given a MISO. Both new runtime paths were compiled for the
chip, not just rendered: a blocking project with `cs="gpio9"` and an embassy one with a
write-only bus and `cs="gpio9"` each `cargo check` cleanly against `esp32c3`.

## 8. Next increment

1. Phase 2 (typed IR), which deletes the wrapper structs, the `*Draft`s and the parallel
   `Vec`s in `Hardware`.

Still open from Appendix E: **Q1 the formatting/drift contract** (needed before Phase 3,
not before these increments), **Q3 where docs prose lives** (today it is inline in
`espf.schema.kdl`, marked as prose), and Q4–Q6, which belong to Phases 2–4.

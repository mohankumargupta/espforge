# Phase 1: one schema (extreme.md §3.2 / §7)

**Status:** increments 1, 1.1, 2 and 3 landed. `espf.schema.kdl` is the source of truth
for the element vocabulary, the attribute rules and the attribute coercion. Nothing in
`espforge-bindings` or `espforge-hardware` parses a schema attribute by hand any more:
`grep -r 'get_attr(' espforge-hardware/src` is empty. Still to do in Phase 1: the merged
options schema and catalog data at build time.

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

Two files, from `espf.schema.kdl`, both plain structs and enums — no macros, so
rust-analyzer and the debugger see ordinary code.

`src/components/generated_vocab.rs` — the `components` elements marked
`bus_component=#true`, in schema order, as `TAGS`.

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

## 5. Gate evidence

- `cargo test --workspace`: green, **287 passed, 1 ignored**.
- The compile gate (`cargo test -p espforge --test golden_projects -- --ignored`): green.
- `espf` CLI transcripts, the diagnostics corpus and the golden projects: unchanged.
  Nothing user-visible moved.
- Generated files are rustfmt-clean and stable: rebuilding produces no diff, and CI now
  checks `generated_elements.rs` alongside the other two.
- New tests: the coercion helpers (`attrs.rs`) and
  `schema_level_problems_keep_their_wording`, which pins the ten messages the generated
  layer sits behind — missing `gpio`/`address`/`cs`/`baud_rate`, a bad hex address, `mode`
  out of range, a bad `parity`, a bad `active_low`, and led/button's
  `needs gpio="$<gpio id>"`. Plus `a_zero_baud_rate_is_refused`, the one rule hardware
  still adds to a coerced value.

## 6. Next increments, in order

1. **Merge `options.kdl` into `espf.schema.kdl`** so there is one file, then generate the
   option types. The catalog loader has to skip `option` nodes; the options loader has to
   skip `section`/`element` nodes; `OPTIONS_KDL` and `SCHEMA_KDL` become one constant.
2. **Catalog data at build time** instead of parsing KDL at runtime, once the schema is
   stable enough that regenerating on every build is cheap.
3. Then Phase 2 (typed IR), which deletes the wrapper structs, the `*Draft`s and the
   parallel `Vec`s in `Hardware`.

Open drift this increment made visible, for a decision before Phase 2:

- **`<spi>` without `miso`.** The schema marks `miso` optional ("displays generally omit
  it"), but `lower_spi` still requires it (`needs a numeric miso="..."`). The generated
  layer follows the schema and yields `Option`, so hardware adds the requirement back
  locally; either the schema is wrong or the lowering is.
- **`<spi_component cs>`** is `kind="str"` in the schema but is a pin in hardware
  (`cs="9"` or `cs="gpio5"`). It wants a `pin`-or-`ref` kind before Phase 2 relies on it.

Still open from Appendix E: **Q1 the formatting/drift contract** (needed before Phase 3,
not before these increments), **Q3 where docs prose lives** (today it is inline in
`espf.schema.kdl`, marked as prose), and Q4–Q6, which belong to Phases 2–4.

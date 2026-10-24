# Phase 1: one schema (extreme.md §3.2 / §7)

**Status:** increments 1, 1.1 and 2 landed. `espf.schema.kdl` is the source of truth
for the element vocabulary, the attribute rules and the attribute coercion, and the six
component wrappers parse no attribute by hand. Still to do in Phase 1: the merged
options schema, catalog data at build time, and letting `espforge-hardware` consume the
wrappers instead of re-reading the same attributes (2b, below).

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

## 3. What was deleted

- `components::TAGS` and `resolve_tag_required_attrs` (hand-written), plus the six
  per-wrapper `REQUIRED_ATTRS` consts and drift test 9 (two hand-written copies of one
  fact).
- The six wrappers' attribute parsing: `get_attr` + `parse` + `trim_start_matches` per
  attribute, `GpioDirection::parse`, `Parity::parse`, `StopBits::parse`, and
  `i2c_component::parse_address` (plus its `parse_i2c_address` re-export). The wrappers
  now keep only the `$`-reference lookup, the slug check and their wording.
- The hand-written enums themselves: `GpioDirection`, `Parity` and `StopBits` are now
  re-exports of the generated enums, so adding a choice to the schema adds a variant.

## 4. Schema changes this increment needed

- `spi.mode` and `spi_component.mode` gained `default="0" range="0..=3"`: the wrapper's
  silent default and its refusal above 3 were rules living only in Rust. They are now
  where the man page reads them, which is the point of the whole exercise. The catalog
  renders the default (no transcript pins the `spi_component` page); `range` is consumed
  by the generator and not rendered yet.
- The header of `espf.schema.kdl` now documents the `attr` properties and both generated
  files.

## 5. Gate evidence

- `cargo test --workspace`: green, **286 passed, 1 ignored**.
- The compile gate (`cargo test -p espforge --test golden_projects -- --ignored`): green.
- `espf` CLI transcripts, the diagnostics corpus and the golden projects: unchanged.
  Nothing user-visible moved.
- Generated files are rustfmt-clean and stable: rebuilding produces no diff, and CI now
  checks `generated_elements.rs` alongside the other two.
- New tests: the coercion helpers (`attrs.rs`) and
  `schema_level_problems_keep_their_wording`, which pins the ten messages the generated
  layer sits behind — missing `gpio`/`address`/`cs`/`baud_rate`, a bad hex address, `mode`
  out of range, a bad `parity`, a bad `active_low`, and led/button's
  `needs gpio="$<gpio id>"`.

## 6. Next increments, in order

1. **2b: hardware consumes the wrapper.** `espforge-hardware/src/components/{led,button}.rs`
   and `{uart,spi}_component.rs` still re-read `active_low`, `baud_rate`, `parity`,
   `stop_bits`, `mode` and `cs` from the node and format their own messages — the second
   vocabulary `improve.md` names. Deleting it changes which sentence a user sees, so it
   needs the message-ownership decision first (Phase 3's codes, or a one-off convergence).
2. **Merge `options.kdl` into `espf.schema.kdl`** so there is one file, then generate the
   option types.
3. **Catalog data at build time** instead of parsing KDL at runtime, once the schema is
   stable enough that regenerating on every build is cheap.
4. Then Phase 2 (typed IR), which deletes the wrapper structs, the `*Draft`s and the
   parallel `Vec`s in `Hardware`.

Still open from Appendix E: **Q1 the formatting/drift contract** (needed before Phase 3,
not before these increments), **Q3 where docs prose lives** (today it is inline in
`espf.schema.kdl`, marked as prose), and Q4–Q6, which belong to Phases 2–4.

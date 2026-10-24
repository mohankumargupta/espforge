# Phase 1: one schema (extreme.md §3.2 / §7)

**Status:** increment 1 landed. `espf.schema.kdl` is now the source of truth for the
element vocabulary, and `espforge-bindings` generates that vocabulary from it at build
time. Everything else in Phase 1 (element types, validation rules, the catalog, the
merged options schema) is still to do.

---

## 1. Decision: how the schema becomes code

Appendix E asked: one KDL file compiled by `build.rs`, or a TOML file read by an `xtask`
that checks in Rust?

**Chosen: KDL → `build.rs` → checked-in Rust**, which is what §3.2 recommends, with two
adjustments:

- No new `xtask` crate yet. `espforge-bindings/build.rs` already generates
  `src/drivers/generated_registry.rs` the same way, so the pattern and its failure modes
  (stale file, IDE confusion) are already known in this repo.
- CI enforces "checked in and current": the `test` job runs `git diff --exit-code` on the
  generated files after `cargo test`, so a schema edit that was not regenerated fails.

Why not `xtask`: it adds a crate and a second command to run; `build.rs` is already wired
into `cargo test`, which is what CI runs. Revisit if codegen grows enough that build times
or rust-analyzer suffer (§8.3).

## 2. What is generated

`espforge-bindings/src/components/generated_vocab.rs`, from the `components` section of
`espf.schema.kdl`:

- `TAGS` — the bus-component tags, in schema order.
- one `*_COMPONENT` const per tag with its required attributes **besides `id`**, and
  `required_attrs(tag)` to look them up.

A "bus component" is derived, not listed: an element in `components` that has a `ref`
attribute to a peripheral (`gpio`, `i2c`, `spi`, `uart`). Adding one to the schema adds it
to `TAGS`; nothing in `espforge-bindings` changes.

## 3. What was deleted

- `components::TAGS` and `components::resolve_tag_required_attrs` (hand-written).
- The six per-wrapper `REQUIRED_ATTRS` consts. Four fed the function above; the
  `led`/`button` two were already dead metadata.
- Drift test 9 (`component_wrappers_agree_with_the_catalog`) — it compared two hand-written
  copies of the same thing. The invariant is now structural.

`espf.schema.kdl` is `elements.kdl` renamed, so the file says what it now is; its loader,
`include_str!` and `cargo:rerun-if-changed` follow the rename, and `SCHEMA_KDL` replaces
`ELEMENTS_KDL`.

## 4. Gate evidence (the Phase 0 net doing its job)

- `cargo test --workspace`: green, 278 passed, 1 ignored (the compile gate, run separately
  and green).
- `espf` CLI transcripts: **byte-identical**, including the whole catalog and the 70-line
  `espf overview`.
- Diagnostics corpus and golden projects: unchanged.
- The generated required-attribute lists match the deleted consts exactly
  (`gpio_component` = `["gpio","direction"]`, `i2c_component` = `["i2c","address"]`,
  `spi_component` = `["spi","cs"]`, `uart_component` = `["uart","baud_rate"]`).

## 5. Next increments, in order

1. **`led` / `button` into the schema.** They are language elements with a `gpio` ref but
   have no catalog entries; adding them changes `espf` output, so it is a deliberate
   catalog diff, not a free refactor.
2. **Generate the element types** (attrs with coercion and spans) and let the wrappers
   consume them, deleting the hand parsing in `espforge-bindings/src/components/*.rs`.
3. **Merge `options.kdl` into `espf.schema.kdl`** so there is one file, then generate the
   option types.
4. **Catalog data at build time** instead of parsing KDL at runtime, once the schema is
   stable enough that regenerating on every build is cheap.

Still open from Appendix E: **Q1 the formatting/drift contract** (needed before Phase 3,
not before these increments), **Q3 where docs prose lives** (today it is inline in
`espf.schema.kdl`, marked as prose), and Q4–Q6, which belong to Phases 2–4.

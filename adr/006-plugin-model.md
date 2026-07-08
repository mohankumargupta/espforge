# ADR-006 — Extension / plugin model

**Status:** accepted

**Decision.** Drivers are declared **one module/file each** via a derive macro
carrying: typed config, `using` name, required features, dependency graph, and a
`generate` body emitting a code fragment. This collapses the current 5-files-across-
4-crates spread to a single declaration. **Discovery = explicit registry list**
(`&[&dyn Driver]` of built-in drivers held by the CLI); the `inventory` +
`black_box` `init()` hack is removed — most debuggable, no link-time magic.
**External/user plugin crates (out-of-tree dynamic discovery) are out of scope for
v1**; drivers ship in-tree and curated.

**Drivers.** The 5-file spread is pure structural overhead with no benefit — config
struct, plugin logic, and runtime impl are one concept. Catalog-driven drivers (B)
can't express bespoke init logic (ssd1306). External crates (C) are wrong for
embedded: no runtime dynamic loading on target, host-side discovery adds a version
matrix for marginal benefit when the driver set is curated.

**Consequences.** Per driver = 1 declaration module; registry is an explicit list.

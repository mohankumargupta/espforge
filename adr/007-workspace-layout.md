# ADR-007 — Workspace & crate layout

**Status:** accepted

**Decision.** Workspace collapses from 13 kind-split crates to **5 role-grouped
crates**:
- `espforge` — CLI binary + parse/resolve/emit orchestration (host/std).
- `espforge-model` — the `DeviceTree` IR, the `Driver` trait, and the explicit
  registry **types** (host/std; depends on neither host nor target).
- `espforge-bindings` — in-tree `generate` impls + the driver registry list
  (host/std). *Devicetree-bindings analogy: contract → codegen glue.*
- `espforge-runtime` — `no_std` runtime implementations of each capability
  (`LED`, `SSD1306Device`, …), **split into distinct `components` and `devices`
  modules**. *Runtime analogy; preserves the component/device distinction esphome
  blurs.*
- `espforge-examples` — sample projects.

**Dependency rule (hard).** `espforge-runtime` depends only on `esp-hal`/
`embedded-hal` (leaf); `espforge-model` depends on neither host nor target; host
crates (`espforge`, `espforge-bindings`) reference `espforge-runtime` *only by
name/path inside emitted token streams* — never link it into the host build. **No
cross-boundary cycles.**

**Runtime module split.** Unlike esphome, `espforge-runtime` keeps **separate
`components` and `devices` modules**, mirroring the three-tier domain spine
(ADR-003). A component capability (I2cDevice, LED, http) lives under `components`;
a terminal device (ssd1306, ili9341) lives under `devices`. This makes the
structural distinction that D3 encodes (devices are terminal) visible in the
runtime layout itself.

**Per-driver file count:** 2 (generate-impl in `espforge-bindings`, runtime-impl in
`espforge-runtime` under its `components` or `devices` module), down from today's 5.

**Drivers.** The dominant constraint is the host/target (std/no_std) wall; a single
mega-crate mixes them and leaks host deps into the target. Grouping by *role*
(model / host-codegen / target-runtime) shrinks the surface and lands the D6
single-declaration cleanly. Keeping the 13-crate kind-split keeps the sprawl that
is a documented cost.

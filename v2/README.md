# espforge v2 workspace

This directory is the **v2** implementation of espforge, built ground-up from a
blank sheet (ADR-011). It is a separate Cargo workspace from the v1 crates at the
repo root, so the two never interfere.

See `../espforge_v2_DESIGN.md` and `../adr/` for the design record (ADR-001..011).

## Crates

| Crate | Boundary | Role |
|---|---|---|
| `espforge-model` | host / std | `DeviceTree` IR, `Driver` trait, VOs (`PinRef`/`ResourceRef`/`Diag`/`Artifact`), registry types |
| `espforge` | host / std | CLI binary + parse/resolve/emit orchestration |
| `espforge-bindings` | host / std | in-tree `generate` impls + driver registry list |
| `espforge-runtime` | target / no_std | runtime impls, split into `components` and `devices` modules |
| `espforge-examples` | CI gate | sample projects |

## Dependency rule (hard, ADR-007)

`espforge-runtime` depends only on `esp-hal`/`embedded-hal`. Host crates
(`espforge`, `espforge-bindings`) reference `espforge-runtime` **only by name inside
emitted token streams** — never as a Cargo dependency. `espforge-model` depends on
neither host nor target. No cross-boundary cycles.

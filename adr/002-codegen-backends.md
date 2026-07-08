# ADR-002 — Codegen backends

**Status:** accepted

**Decision.** espforge has exactly **one backend: Rust firmware** (blocking +
embassy are *modes* of that backend, not separate backends). The architecture
provides a single inspectable **IR** with **multiple emitters** (Rust sources,
`Cargo.toml`, wokwi `diagram.json`, JSON dump, DOT graph) — these are *artifacts of
one backend*, not language backends. A **multi-language backend** (Zig / MicroPython
/ C) is explicitly a **non-goal for v1**; no `Backend` trait is erected.

**Drivers.** No concrete second backend named with a near-term driver. A
Rust-coupled component library (wraps `esp_hal`) cannot be reused by another
language, so a present-day `Backend` trait would encode a fiction. The IR remains
the future seam if a real second backend ever appears.

**Consequences.** Emitters are `fn(&DeviceTree) -> Result<Artifact>`; adding an
artifact type cannot break the Rust emitter.

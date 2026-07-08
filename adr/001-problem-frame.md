# ADR-001 — Problem frame & source of truth

**Status:** accepted

**Decision.** espforge is a YAML/`app.rs`-driven generator + maintainer. Source of
truth = {project YAML, `app.rs`, `dependencies.toml`, optional `.cargo/config.toml`
override, optional `diagram.json` override}. All other project files are generated.

**Boundary enforcement.** espforge emits an **enforcement-grade** ownership
manifest (input files + checksums, set of owned files) and a human `README.txt`.
Layered files (`.cargo/config.toml`, `diagram.json`): if a user copy exists in the
source-of-truth dir, it **fully replaces** espforge's generated base (binary
ownership, no merge). Regeneration is idempotent and drift-detecting; a checksum
mismatch on an owned file → espforge refuses rather than clobbers.

**Drivers.** Target user edits a declarative spec, not boilerplate. ESP32
peripherals are owned singletons best wired statically. The existing regen
machinery is only justified if regeneration is a first-class operation.

**Consequences.** Generated files never hand-edited; `Cargo.toml` generated, user
deps via `dependencies.toml`; regeneration idempotent + drift-detecting.

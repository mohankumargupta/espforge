# ADR-009 — Validation & diagnostics

**Status:** accepted

**Decision.** A single **`validate` stage runs before `resolve`/`emit`** and gates
`compile`. It checks: unknown drivers, missing/unresolved `$name` refs, `with:`
type mismatches, **pin/peripheral double-claims** (catches ADR-008-B's risk at YAML
time), dependency cycles, and required-feature/dependency coherence. Config errors
are **span-aware, structured `Diag`** (file:line:col + field path + fix hint), not
bare `anyhow` strings; `anyhow` is reserved for pipeline/I/O failures only. A
**`espforge validate`** subcommand runs the same stage and reports without emitting.

**Drivers.** The IR (ADR-005) exists to be the trustworthy artifact emitters
consume; if invalid config can reach `resolve`/`emit`, the IR can't be trusted and
emitters must defensively `unwrap` (today's late, location-less failures). Span-
aware `Diag` is the point of a user-facing generator. This also catches ADR-008-B's
double-claim risk cheaply. `validate` as a subcommand shares the same stage without
emitting; validation must gate `compile`, not be optional.

**Consequences.** IR is valid-by-construction; emitters and move-by-value runtime
stay clean.

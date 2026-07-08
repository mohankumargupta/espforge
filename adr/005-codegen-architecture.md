# ADR-005 — Codegen architecture

**Status:** accepted

**Decision.** Generation is a **pure staged pipeline**: `parse → validate →
resolve(IR) → emit*`. The dedicated **`DeviceTree` IR** (produced by `resolve`)
carries: project metadata, typed peripherals, typed instances (resolved `with:`
params + dependency edges with kinds/access-paths), `init_order`,
`required_features`, and flags (is_embassy, has_alloc, has_wifi, needs_delay,
needs_stack). **All emitters are pure `fn(&DeviceTree) -> Result<Artifact>`** where
`Artifact` is an in-memory file (path + content); a thin I/O layer writes them and
the ownership manifest records them. Emitters read only the IR, never the raw
config.

**Drivers.** The single documented pain is the untestable monolith and fail-late
validation. A pure staged pipeline with an IR directly fixes both: stages are
pure `fn`s, validation becomes a stage before codegen. The IR is the seam ADR-002's
multi-emitter needs. Keeping `EspforgeConfiguration` as the IR leaks parse-time
concerns into codegen.

**Consequences.** Emitters pluggable and isolated; parse/codegen/config unit-
testable (ADR-010); IR is valid-by-construction after validate (ADR-009).

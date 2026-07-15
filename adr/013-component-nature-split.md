# ADR-013 — Hardware vs software-service component split (user-invisible)

**Status:** accepted (decided 2026-07-16, grilling session)

## Context

The model already distinguishes components by *behavior*: hardware-backed
components (`i2c`, `spi`, `uart`, `led`, `button`) declare `pins`/`peripherals`
and participate in the `claimed_by` graph; software-service components (`http`,
future `mqtt`/`websockets`) declare only cross-cutting flags (`is_embassy`,
`has_wifi`, `needs_stack`, `has_alloc`) and consume the implicit Stack (ADR-012).
The question: should this distinction be exposed to the **user**, and if not, how
should it be encoded?

Relevant facts:
- esphome does **not** distinguish HW vs SW components in user YAML — users pick
  a component kind and the system handles claims/infra internally.
- In v2, a user adds `http` and `i2c` the *same way*: pick a `using:` kind,
  optionally fill `with:`. There is **no user decision that differs** between
  the two — the claim machinery vs flag machinery handles the rest.
- The model's ethos (ADR-012, §2b) is "convention in the data, not annotation."

## Decision

**The distinction is user-invisible.** Keep a single unified `components:` key in
user YAML and a single unified generated `Context` field (`ctx.components.x`, so
app code like `ctx.components.http` is also unaffected). Users never see the words
"service" or "hardware."

The `nature` is **derived, not annotated**: a component is `Service` iff it
declares no `pins` and no `peripherals` (only flags); otherwise `Hardware`. This
reuses the existing `claimed_by` data, cannot drift, and needs no new schema
surface. (The Stack is the one allowed exception: a `Service` like `http` asserts
`needs_stack` but claims no peripheral — already carved out in ADR-012 §2b.)

The split exists **only internally**, in two places where it has consequences:
1. **Driver authoring:** runtime source layout splits into
   `espforge_runtime/src/components/` (hardware: `i2c`, `spi`, `uart`, `led`,
   `button`) and `espforge_runtime/src/services/` (`http`). Pure code
   organization; no user or app impact.
2. **Validation:** the validator enforces `Service ⇒ declares no pins/peripherals`
   (already applied to `http` in ADR-012), and `Hardware ⇒ claims ≥1 peripheral
   or pin`. This is where the distinction earns its keep — catching driver
   authoring mistakes, not user input.

The emitter already lets each driver emit its own `construct` and routes the
Stack/`&'static Stack` plumbing from `needs_stack` flags, so no codegen change is
required to honor the split.

## Consequences

- Zero cognitive load on users (matches esphome).
- No new YAML namespace, no new schema field, no new context field.
- Driver authors get a structural home (`services/`) and an enforcement rule;
  the distinction is observable exactly where it matters.
- App code stays uniform (`ctx.components.*` for everything).

## Alternatives considered

- **A: New top-level `services:` YAML key** (separate namespace from `components:`).
  Rejected: it would be pure cognitive tax — a classification the *system* cares
  about, not the user. A user choosing `using: http` makes the same decision as
  choosing `using: i2c`; exposing a namespace adds surface with no new capability
  and leaks the distinction into user YAML that esphome keeps hidden.
- **B: Annotate each driver with a manual `nature: hardware|service` tag.**
  Rejected: manual annotation can drift, and the data already implies it (flags
  vs pins/peripherals). Derivation is cheaper and self-correcting.
- **C: Split the generated `Context` into `ctx.components` + `ctx.services`.**
  Rejected: that leaks the distinction back into app code, defeating the
  user-invisible goal. Kept unified; revisit only if app authors explicitly want
  to *see* which capabilities are infrastructure.

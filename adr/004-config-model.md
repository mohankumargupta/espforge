# ADR-004 — Configuration model

**Status:** accepted

**Decision.** Typed sectioned YAML — `espforge:` (name/platform/runtime),
`esp32:` (peripherals), `components:` (`using:` + `with:`), `devices:` (same).
`using` selects a driver; `with:` is deserialized into the driver's typed schema
and validated up front (not re-parsed as raw `Value`). References use `$name`,
normalized to typed `PinRef`/`ResourceRef` at deserialization (esphome-like,
familiar). **App logic lives only in `app.rs`** — YAML is structure-only, no
declarative-step DSL. No section renames.

**Drivers.** The sectioned model maps 1:1 to the 3-tier spine users think in.
Flattening to a single list loses ergonomic grouping; alternate sigils trade away
esphome familiarity for marginal safety already provided by typed refs. Keep the
explicit-over-implicit philosophy: addresses, active-low, frequencies stay in YAML.

**Consequences.** Each component/device declares a schema; `with:` validated at
parse time (fixes fail-late validation). Config model is purely declarative.

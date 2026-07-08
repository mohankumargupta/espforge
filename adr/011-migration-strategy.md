# ADR-011 — Migration strategy

**Status:** accepted

**Decision.** Migration is a **clean-slate big-bang on a new branch `espforgev2`**,
built ground-up from a blank sheet — *not* an in-place strangler on the existing
13-crate repo. The old repo is left intact (continues as v1); `espforgev2` is the
fresh implementation of ADR-001–010. **User YAML is unchanged** (ADR-004: sections +
`$name` + `app.rs` identical), so existing projects carry over without edits; the
cost of the big-bang is borne entirely internally (no dual-codebase maintenance in
one binary). The rewrite's exit criterion: `espforgev2` must reproduce the example
outputs and pass `espforge validate` on all `espforge_examples`.

**CLI surface includes `validate`** (ADR-009) **and `version`** (prints the espforge
version) as first-class subcommands.

**Drivers.** User YAML is the only external contract; a new engine is invisible to
users as long as it emits the same project tree shape. A strangler (A) and per-driver
port (C) were considered; user opted for the clean-slate rewrite to avoid dragging
the old `inventory`/5-file structure along. Big-bang risk is acceptable because the
YAML contract is stable and no live dual-codebase is maintained in one binary.

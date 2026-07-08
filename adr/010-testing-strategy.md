# ADR-010 — Testing strategy

**Status:** accepted

**Decision.** Primary testing = **A**: stage-level unit tests on the pure `fn`s
(`parse`/`validate`/`resolve` → `DeviceTree`) + IR/token golden tests, all host-side
and hermetic. **Discipline: tests are written when an actual bug is detected**
(regression tests), not speculatively — the pure-pipeline design makes this cheap to
do on demand. `espforge_examples` `cargo build` is retained as a **CI integration
gate** (catches target-compile breakage) but is not the primary test. Mock-HAL
runtime tests (C) deferred to a post-v1 optional layer.

**Drivers.** A is the direct dividend of the ADR-005 pure pipeline: pure `fn`s are
trivially testable and the IR is the perfect assertion target (parse YAML → assert
`DeviceTree`; run emitter → assert source) without filesystem or target. Example
builds (B) are too slow/opaque to be primary. Mock-HAL (C) needs a mock `esp-hal`
surface and only tests runtime, not codegen — valuable later, not a v1 blocker.

**Consequences.** All A tests run on host (std); target crates tested via CI example
builds or future mock-HAL, never linked into host unit tests.

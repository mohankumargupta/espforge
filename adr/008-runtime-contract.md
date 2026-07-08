# ADR-008 — Runtime contract & pin ownership

**Status:** accepted

**Decision.** Runtime pin/peripheral ownership uses **move-by-value (B)**:
`PeripheralRegistry` yields the *specific* typed peripherals each component needs,
by value, into generated per-project `Components::new(...)` / `Devices::new(...)`
signatures produced by the IR. **No `RefCell`, no `Option`, no `take().unwrap()`.**
The typed IR (ADR-005) moves each peripheral exactly once; the compiler *statically*
forbids double-claim (a YAML double-claim surfaces as a generated-code compile
error, and is caught earlier by D9's validate stage). Embassy `Stack<'static>` is
passed as a borrow alongside owned peripherals.

**App-facing `Context` stays stable:** `{ logger, delay, component!()/device!()
accessors }` — app code shape unchanged even though internal wiring signatures are
generated per-project.

**Bus-sharing stays at the component tier:** an `I2cDevice`/`SpiDevice` component is
shared by reference by many devices; the registry itself need not support sharing.

**Scope of assurance.** Sound for the current esp-hal peripheral model (owned,
`'static`, movable types). A future borrowed-only HAL peripheral would degrade to
`RefCell<Option>` for that one peripheral only, not the whole system.

**Drivers.** A's `RefCell<Option>` + `take().unwrap()` is runtime enforcement of a
compile-time-known fact (each pin claimed by exactly one component, known at codegen
via the IR). B makes invalid states unrepresentable. C (shared-borrow at registry)
is unnecessary because bus-sharing already lives at the component tier.

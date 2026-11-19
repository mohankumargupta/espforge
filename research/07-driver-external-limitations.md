## external drivers limitations


- External manifests can only describe bus-backed devices on three fixed transport types. The manifest parser accepts only i2c, spi, and uart; lowering and emission then branch on those same fixed kinds. Adding a new transport, a custom peripheral element, or a new driver lifecycle requires changes across the schema parser, board model, lowerer, and emitter—not just a manifest. See driver.rs (espforge-schema/src/driver.rs:634), recipe.rs (espforge-elements/src/devices/recipe.rs:511), and model.rs (espforge-elements/src/model.rs:1248).
- The extension mechanism is declarative rather than a public plugin API. A project manifest can compose a schema DeviceRecipe, but the available recipe fields and constructor argument forms are defined by the compiler. Behaviors outside those fields still need repository changes and, for hand-written elements, registration in the workspace’s distributed slice. That makes ordinary sensor/display drivers approachable, while unusual initialization flows or device interactions are harder to add independently. See schema.rs (espforge-elements/src/schema.rs:115) and element.rs (espforge-elements/src/element.rs:84).

The current driver format covers a large, useful slice of sensors: the bundled manifests already show I²C, SPI, UART, configurable attributes, control pins, and deferred initialization. Standard I²C/SPI devices with a Rust crate that provides a constructor matching the manifest model are usually a good fit.

It falls short when the sensor needs a connection or lifecycle the format can’t represent:

| Example | Where the current model falls short |
|---|---|
| DS18B20 temperature probes | The DS18B20 uses 1-Wire, including precise timing and multidrop discovery on a shared data line. Driver manifests only accept I²C, SPI, or UART; GPIO pins are auxiliary constructor arguments, not a general bit-banged bus. Supporting this as a proper bus would require compiler and runtime work. Datasheet (https://www.analog.com/media/en/technical-documentation/data-sheets/DS18B20.pdf) |
| NTC thermistors or other analog sensors connected directly to the MCU | These require configuring and sampling the chip’s ADC. The manifest model has no ADC transport or analog-input element. An external I²C ADC such as ADS1115 is a better fit; ESPForge already has an ads111x manifest. |
| HX711 load-cell modules | The interface is a clock line plus data line with protocol-specific timing. It doesn’t fit the existing bus types, and two ordinary pin arguments don’t give ESPForge a reusable, shared bus or timing-aware runtime. |
| SDI-12 environmental sensors | SDI-12 uses a timed, half-duplex single-wire interface. It isn’t among the supported transports, and the schema has no way to configure that transport’s signaling or timing. |
| BMI270-class IMUs with sensor-specific firmware loading | A BMI270 needs an initialization sequence that uploads an 8 KB configuration image, then waits for the sensor to report that loading completed. A manifest describes a constructor call; it can’t emit that sequence. It works only if a driver crate wraps the whole sequence in a compatible constructor. Otherwise, this needs custom implementation or richer initialization support. Bosch datasheet (https://www.bosch-sensortec.com/media/boschsensortec/downloads/datasheets/bst-bmi270-ds000.pdf) |
| Drivers with builder or staged initialization APIs | The manifest constructor must be one call with positional arguments. It can map attributes and pass fixed expressions, but can’t generate a builder chain, call init() afterward, or insert arbitrary setup steps. A crate with that API needs a wrapper crate or a new code path. |
| Sensors that share a UART bus or need multiple peers on one UART | UART is modeled as a point-to-point port owned by one device; lowering refuses a second device on the same port. Individual UART sensors can fit, as the mhz19b manifest demonstrates, but shared or multi-drop UART arrangements cannot use this model. |
| Deeply composed sensor assemblies | A manifest can compose companion drivers, but composition is limited to one level: a composed part cannot itself compose another driver. Assemblies that need nested components must be flattened or handled in custom code. |

Some apparent gaps depend on the driver crate. An interrupt pin, reset pin, address choice, mode setting, or calibration parameter can often be handled if the crate’s constructor accepts it. The harder cases are where the API requires steps after construction, a new transport, ADC access, or shared bus behavior.

The main boundaries are implemented in the manifest parser, which restricts bus kinds and constructor shape (driver.rs (espforge-schema/src/driver.rs:634), constructor parsing (espforge-schema/src/driver.rs:1671)), and in the runtime’s fixed transport variants (model.rs (espforge-elements/src/model.rs:1248)). MAX31855 thermocouple converters, by contrast, use a simple SPI-compatible interface and look like a strong fit for this model. Analog Devices overview (https://www.analog.com/en/products/max31855.html)

# Driver extensibility roadmap for v0.10.0

**Status:** proposed roadmap  
**Baseline:** workspace `0.9.1`; v2 driver manifests, dependency feature selection, fixed constructor expressions, generic endpoint types, infallible constructors, and deferred initialization are already present. `driverfixes.md` documents that groundwork.  
**Goal:** let a substantially broader set of common sensors be added as project or published driver packages, without requiring an `espforge-elements` Rust implementation for each part.

This plan focuses on the gaps identified in the driver extensibility review: transports beyond I²C/SPI/UART, direct analog sensing, 1-Wire and GPIO-timed protocols, staged initialization, interrupt/data-ready wiring, UART sharing policy, and limits in sensor composition. A driver crate that already hides a sensor's complexity behind a compatible constructor can be supported today; the items below address cases where the host-side manifest and generated runtime cannot express the hardware or lifecycle.

## Product boundary

The manifest is a typed description of a device constructor. It can name a crate, map attributes to Rust types or expressions, claim auxiliary pins, and choose boot or deferred construction. It is not a general embedded workflow language. New work should keep transport and lifecycle behavior in `espforge-runtime` and the device crate, with the manifest selecting validated options.

For v0.10.0, prioritize two new usable paths:

1. Direct ADC inputs, including common analog sensors and thermistors.
2. One-wire devices, including DS18B20 probes and multidrop networks.

Also make constructor lifecycle limitations explicit, prevent awkward UART behavior, and improve reusable pin and integration support. SDI-12, arbitrary bit-banged protocols, and nested composite expansion should be designed as follow-on work unless a concrete driver package proves they fit safely in this release.

## Current architecture and constraints

- `espforge-schema/src/driver.rs` parses the v2 `driver` manifest. `parse_bus` currently accepts only `i2c`, `spi`, and `uart`.
- `espforge-elements/src/devices/recipe.rs` lowers recipe devices, claims bus and GPIO resources, checks address/CS constraints, and constructs `BusTransport`.
- `espforge-elements/src/model.rs` has closed `Bus`, `BusTransport`, and peripheral model enums. Adding a transport therefore requires coordinated model, lowering, and emitter changes.
- `espforge-elements/src/bus_emit.rs` constructs chip buses and device endpoints. `espforge-emit/src/codegen/rust.rs` assembles generated device fields, factories, and app context.
- `espforge-runtime/src/lib.rs` already exposes `Bidirectional` for a GPIO line that drivers can drive and sample. That is a pin wrapper, not a general timed bus or multidrop manager.
- UART device recipes bind one device to one UART port; the lowerer refuses a second device on that port.
- `compose` lowers companion devices one level deep. A composed part that itself has parts is rejected.
- External driver crates can implement complex behavior inside their own `new()` today. The manifest cannot emit a builder chain or multiple post-construction setup calls.

## GitHub Project fields

Use these fields on each issue/card:

| Field | Values / convention |
|---|---|
| **Status** | `Backlog`, `Ready`, `In progress`, `In review`, `Done` |
| **Milestone** | `v0.10.0 / Discovery`, `v0.10.0 / ADC`, `v0.10.0 / OneWire`, `v0.10.0 / Driver polish`, or `v0.11.0 / Protocol expansion` |
| **Priority** | `P0` release blocker, `P1` core v0.10 scope, `P2` follow-on |
| **Area** | `schema`, `elements`, `runtime`, `emit`, `CLI/docs`, `integration` |
| **Labels** | `drivers`, one area label, and `breaking-change` only if the work changes existing generated APIs |

Issue titles below use GitHub style (`[Area] Imperative work item`). Each checklist is intended to become one or more issue acceptance criteria. Keep issues small enough to review and close independently; use the listed dependencies to order the project board.

---

# Milestone: v0.10.0 / Discovery

### [Drivers] Publish and apply a sensor coverage matrix

**Priority:** P0 · **Area:** CLI/docs, integration · **Estimate:** S

Create a checked-in matrix of representative sensor families and classify each as `manifest-only`, `wrapper crate needed`, or `runtime/compiler work needed`. Include common environmental sensors, IMUs, thermocouple converters, analog sensors, 1-Wire probes, load cells, and UART sensors. Record bus, constructor shape, special pins, init behavior, crate/license status, and a generated-project compile result.

**Acceptance criteria**

- The matrix distinguishes devices that fit the current manifest from transport/lifecycle gaps; it does not label every hard-to-use crate a compiler limitation.
- Include at least one simple reference device per v0.10 transport path and explicit examples for DS18B20, an ADC-backed thermistor, and a standard I²C/SPI sensor.
- Identify one candidate no-std Rust crate for each v0.10 vertical slice before finalizing its public API.

### [Runtime] Specify transport, pin ownership, and deferred-init contracts

**Priority:** P0 · **Area:** runtime, elements · **Estimate:** M

Write a short internal design note before coding the ADC and OneWire slices. Define how resources are claimed, whether a bus may be shared, what the manifest declares, what generated code constructs, and which types a driver crate receives. Preserve the property that duplicate or incompatible use becomes a located diagnostic rather than a generated Rust error.

**Acceptance criteria**

- The note covers ownership/lifetimes, blocking and embassy behavior, cancellation or lock behavior for async operations, and deferred initialization.
- It explains how chip capability metadata is validated (for example, which pins support ADC on each chip).
- It states how raw access and recipe access interact, including whether an app can also claim a declared peripheral.

---

# Milestone: v0.10.0 / ADC

## Phase A — Direct analog input vertical slice

### [Schema] Add an ADC peripheral and analog channel declarations

**Priority:** P1 · **Area:** schema · **Estimate:** M

Add an `adc` peripheral model distinct from the current I²C/SPI/UART bus types. Let the spec declare an ADC peripheral and one or more input channels by GPIO. The exact ESPF shape should follow the discovery note; a likely shape is `# peripherals` entries that declare an ADC unit plus channel IDs and pins. Do not model an analog channel as an ordinary digital `<gpio>` reference.

**Implementation notes**

- Add schema definitions and generated attribute readers in `espforge-schema/src/espf.schema.kdl` and `espforge-elements/build.rs` inputs as needed.
- Add typed IR variants and ownership records in `espforge-elements/src/model.rs` and lower them in a focused peripheral module.
- Validate chip-specific ADC units, channel/pin compatibility, attenuation, resolution, and duplicate channel claims from metadata rather than hard-coded manifest assumptions.
- Keep per-sensor conversion/calibration policy in driver crates or explicit attributes; do not bake thermistor equations into the core.

**Acceptance criteria**

- Invalid chip/pin/channel combinations receive a located diagnostic.
- Two declarations cannot claim the same ADC channel or physical pin incompatibly.
- The core can represent an ADC input without falsely treating it as a bus endpoint.

### [Runtime] Emit ADC channels with blocking and embassy read support

**Priority:** P1 · **Area:** runtime, emit · **Estimate:** L

Implement the ADC channel wrapper and generated construction path. Expose a stable runtime type usable by external crates and/or generated app code. If esp-hal ADC reads are synchronous only, document whether embassy projects use a synchronous sample inside a task or whether the runtime offers an async wrapper; do not claim async support without a defined executor-safe ownership model.

**Implementation notes**

- Add ADC runtime feature wiring and chip-gated `esp-hal` dependencies in `espforge-runtime/Cargo.toml`.
- Add an explicit model and emitter path rather than forcing ADC into `BusTransport`.
- Decide whether a channel is shared behind a lock or exclusively owned. Encode that choice in the IR and generated API.
- Ensure the channel can be moved into a sensor driver while preventing duplicate raw access.

**Acceptance criteria**

- Generated code builds on at least one supported RISC-V chip and one supported Xtensa chip with ADC enabled.
- A generated project can sample an ADC channel and use the result from `app.rs`.
- Projects without ADC do not acquire ADC features or unnecessary code.

### [Drivers] Add the first ADC-backed sensor manifest

**Priority:** P1 · **Area:** schema, integration · **Estimate:** M · **Depends on:** ADC peripheral and runtime issues

Add one real, small vertical slice: a thermistor/analog sensor adapter or a no-std driver crate that consumes the new ADC channel abstraction. Prefer a simple voltage/temperature example to validate the API before attempting arbitrary ADC algorithms.

**Acceptance criteria**

- The driver is declared without a hand-written `Element` implementation.
- Its manifest validates channel selection, allowed options, and any calibration values.
- The generated project compiles and demonstrates a read in `app.rs`.
- The example documents electrical assumptions (divider/reference/attenuation) without silently choosing hardware wiring.

### [Drivers] Add analog-sensor diagnostics and examples

**Priority:** P1 · **Area:** elements, docs · **Estimate:** S · **Depends on:** ADC issues

Provide examples for pin/channel binding and diagnostics for unsupported pins, duplicate channel use, and invalid attenuation/resolution values.

---

# Milestone: v0.10.0 / OneWire

## Phase B — One-wire multidrop vertical slice

### [Schema] Add a shared one-wire bus declaration

**Priority:** P1 · **Area:** schema · **Estimate:** M

Add a first-class one-wire peripheral rather than pretending a one-wire line is I²C or passing an arbitrary GPIO to every sensor. A bus declaration owns one bidirectional data pin and records its pull configuration and timing/runtime mode. Driver devices reference the bus; multiple addressed devices may share it.

**Implementation notes**

- One-wire line timing is protocol behavior. The bus should be backed by a reusable runtime/driver abstraction, not hand-coded in every sensor manifest.
- Add explicit support for device ROM identifiers or discovery. Do not reuse the I²C `address` field: a 1-Wire ROM code is not a 7-bit address.
- Define whether parasite-powered devices are in scope. Strong pull-up and conversion wait requirements may need a bus capability or an explicit unsupported diagnostic in v0.10.0.
- `Bidirectional` exists in `espforge-runtime`, but the one-wire bus needs timing, arbitration, reset/presence detection, and multidrop semantics in addition to that pin wrapper.

**Acceptance criteria**

- One bus can serve multiple one-wire devices without duplicate peripheral construction.
- Resource checking prevents the data pin from being simultaneously assigned to an incompatible owner.
- The manifest can select a device by ROM ID or documented discovery policy.
- Unsupported parasite-power or timing modes fail with an actionable message.

### [Runtime] Implement the OneWire bus adapter

**Priority:** P1 · **Area:** runtime · **Estimate:** L · **Depends on:** OneWire schema issue

Implement the timing- and GPIO-specific OneWire master using the selected no-std crate or the `esp-hal` GPIO/timer primitives. Keep pin manipulation behind a runtime abstraction that can be used by external sensor crates. Ensure critical timing sections cannot be preempted in a way that corrupts slots; specify behavior under embassy.

**Acceptance criteria**

- Reset/presence, bit read/write, ROM search, and CRC behavior are covered by unit or hardware-in-loop checks appropriate to the chosen implementation.
- Timing constraints and unsupported clock/runtime combinations are documented and checked.
- Multiple devices on one bus share one master and do not each consume the GPIO independently.

### [Elements] Lower one-wire devices and validate ROM selection

**Priority:** P1 · **Area:** elements · **Estimate:** M · **Depends on:** OneWire schema and runtime issues

Extend the driver recipe with a one-wire transport and a ROM/discovery selection model. Add duplicate ROM checks where IDs are explicit. A recipe should not be forced to declare an I²C address or SPI chip-select attribute.

**Acceptance criteria**

- The lowerer resolves the bus reference against a one-wire peripheral and rejects wrong transport types.
- ROM IDs are parsed and validated at check time.
- Address uniqueness checks remain transport-specific; I²C collision logic does not run for one-wire IDs.

### [Drivers] Add DS18B20 as the OneWire acceptance driver

**Priority:** P1 · **Area:** schema, integration · **Estimate:** M · **Depends on:** OneWire vertical slice

Add a DS18B20 manifest backed by a compatible Rust crate. Include resolution, optional ROM selection, conversion timing, and temperature read examples to the extent the crate API exposes them. Keep a second probe on the same bus in an example to prove multidrop support.

**Acceptance criteria**

- No custom tag-specific Rust element is required.
- A project with two DS18B20s on one bus generates one bus master and two sensor devices.
- ROM selection and unsupported parasite-power behavior are explicit.
- At least one generated project compile check and one hardware smoke test are recorded.

---

# Milestone: v0.10.0 / Driver polish

## Phase C — Pin events and staged construction

### [Schema] Add explicit interrupt/data-ready pin semantics

**Priority:** P1 · **Area:** schema, elements · **Estimate:** M

Auxiliary `pin` declarations currently create input/output/bidirectional wrappers from static settings. Add an explicit interrupt/data-ready role only if a driver needs an interrupt-capable line that the app/runtime can await or route. Specify edge/level, polarity, optionality, and whether one GPIO can be shared.

**Implementation notes**

- Avoid assuming every sensor IRQ is the same: edge-triggered data-ready, latched active-low alert, and wake-from-sleep have different lifecycle requirements.
- Decide whether the generated device retains an interrupt handle or whether an application task subscribes through `Context`.
- The design must define interrupt setup ordering relative to deferred `.init()`.

**Acceptance criteria**

- A representative IMU data-ready driver can declare and consume its IRQ without hand-written element code.
- The app has a documented way to wait for/read events without busy polling.
- Pin ownership and interrupt routing conflicts are diagnosed.

### [Elements] Add staged initialization hooks only for proven crate APIs

**Priority:** P2 · **Area:** schema, elements, emit · **Estimate:** L

The current constructor template is one function call. Before adding a general builder DSL, survey actual driver crates and implement the smallest typed lifecycle model that covers them: for example, `construct -> async/sync init -> ready device`, with each stage's error mapped to `DeviceError` and deferred factories retaining the required arguments.

**Non-goals**

- Do not accept arbitrary Rust blocks or source snippets in KDL.
- Do not add builder-chain syntax unless multiple real drivers need it and the syntax can be parsed and validated safely.
- Do not move sensor-specific configuration sequences into core lowering.

**Acceptance criteria**

- At least two real crate APIs demonstrate the same lifecycle extension need before a generic feature is accepted.
- Blocking and async stage order is checked at manifest load time.
- Failure at each stage reports the device id and an initialization error.
- Boot and deferred paths have compile coverage.

### [Elements] Define UART sensor sharing and raw-port ownership

**Priority:** P1 · **Area:** elements, runtime · **Estimate:** M

The current UART transport hands the whole port to exactly one device, and the lowerer refuses a second bind. Keep exclusive ownership as the safe default. Document it and add a deliberate shared-UART abstraction only if a concrete supported protocol requires multiple logical devices or a bus multiplexer.

**Acceptance criteria**

- One UART-backed sensor continues to work in blocking and embassy modes where its crate supports them.
- A second consumer gets a diagnostic that explains exclusive port ownership and the available remedy.
- If shared UART is approved, framing, synchronization, request/response arbitration, timeouts, and ownership are implemented in one shared runtime object; manifests must not create two UART peripherals.

## Phase D — Composition and external package ergonomics

### [Schema] Specify and bound composite-driver expansion

**Priority:** P2 · **Area:** schema, elements · **Estimate:** M

Current `compose` is intentionally one level deep. Decide whether the common use cases require nested composites. If so, support recursive expansion with cycle detection, deterministic generated IDs, and a maximum expansion bound; otherwise improve diagnostics and document flattening. Do not enable recursion without cycle protection.

**Acceptance criteria**

- A cycle such as A -> B -> A is rejected at manifest load with the participating tags.
- Expansion order and generated subdevice IDs are deterministic.
- Duplicate IDs, pins, I²C addresses, and driver features are checked across the fully expanded graph.
- Existing one-level composites preserve their generated output.

### [Drivers] Add integration fixtures for external sensor crates

**Priority:** P1 · **Area:** integration · **Estimate:** M

For every newly supported API shape, maintain a small local fixture crate and a driver manifest. Use it to verify dependency path resolution, generic endpoint types, feature selection, deferred init, and generated `Cargo.toml` merging without relying on a network registry during routine checks.

**Acceptance criteria**

- Fixture generation is offline and deterministic.
- At least one fixture exercises a constructor error, one an infallible constructor, and one a deferred/async path where applicable.
- Diagnostics point to the manifest or spec location for invalid bindings.

### [Docs] Publish a driver author guide and capability table

**Priority:** P1 · **Area:** CLI/docs · **Estimate:** S

Update `docs/002-external-driver-manifest.md` with the current feature boundary and examples for ADC and OneWire once implemented. Include a compact “fits today / needs runtime support” table and a recipe for testing a fetched/local driver package.

**Acceptance criteria**

- Each supported transport has a complete manifest example.
- The guide explains limitations around staged constructors, pin interrupts, UART exclusivity, composite depth, and ADC capability validation.
- The sensor coverage matrix from Discovery links to working examples.

---

# Milestone: v0.11.0 / Protocol expansion

### [Runtime] Evaluate SDI-12 as a dedicated protocol package

**Priority:** P2 · **Area:** runtime, schema · **Estimate:** L

Do not add SDI-12 as a generic UART alias. Evaluate a dedicated half-duplex transport or a separately packaged protocol driver that owns its electrical interface, timing, wake/break signaling, and bus arbitration. Add it only with a concrete sensor crate and hardware target.

### [Runtime] Evaluate reusable GPIO-timed bus support

**Priority:** P2 · **Area:** runtime · **Estimate:** XL

Compare dedicated protocol peripherals against a generic timing engine. A generic GPIO-timed bus has strict scheduling, interrupt, and critical-section implications; avoid presenting it as a simple manifest pin option. Include protocols such as HX711-style clock/data and other one-wire schemes in the evaluation, but do not merge electrically incompatible protocols into one abstraction.

### [Drivers] Expand coverage from the v0.10 sensor matrix

**Priority:** P2 · **Area:** integration · **Estimate:** ongoing

Use v0.10 experience to add common I²C/SPI/UART drivers as manifests first. Promote a new transport or lifecycle capability only when several real devices need it and the generic API can be validated independently of one vendor crate.

---

## Cross-cutting engineering checklist

Every issue that changes the driver pipeline should account for the relevant items below:

- **Schema:** validate transport-specific attributes and placeholders at manifest load time; preserve clear diagnostics and duplicate-tag behavior.
- **IR:** model ownership and transport as explicit typed variants; avoid unrelated optional fields whose combinations can be invalid.
- **Lowering:** validate chip capabilities, pin direction, resource conflicts, addresses/identifiers, runtime support, and duplicate consumers before emission.
- **Emission:** construct each physical controller once; keep generated names deterministic; preserve boot/deferred semantics and map errors to the device ID.
- **Cargo:** keep driver crate features and runtime features minimal; test version/path/git dependency merging and blocking/async feature combinations.
- **API stability:** generated code is user code. If a new runtime type changes public generated fields or initialization signatures, flag the release item as a breaking change and document migration.
- **Offline behavior:** `check`, `gen`, and build must continue to consume resolved manifests/dependencies without fetching driver sources unexpectedly.
- **Documentation:** include an ESPF snippet, wiring assumptions, runtime support, and at least one working app usage example for each new transport.

## Suggested release gates

1. Discovery matrix and transport contracts reviewed before implementation.
2. ADC vertical slice works end to end, or is explicitly removed from v0.10.0 scope before the release candidate.
3. OneWire vertical slice works end to end with two devices on one bus, or is explicitly deferred before the release candidate.
4. All existing I²C/SPI/UART golden projects retain their expected output unless a documented API change is approved.
5. Driver author guide and coverage matrix match the released implementation.

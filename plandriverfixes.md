# `plandriverfixes.md`: Phased Plan for Busless, 1-Wire, Analog and Codegen Gaps

**Status:** implementation plan
**Baseline:** `espforge` workspace `0.9.2`, branch `dev`
**Supersedes:** the phasing in `drivergaps.md` (same gap audit, different order and four corrected items — see §2)
**Purpose:** support the device families `drivergaps.md` names, in the order that produces working devices fastest and with the least rework.

---

## 0. The order, and why

| Phase | Unlocks | Gaps closed | Why here | Size |
|---|---|---|---|---|
| **0** | (nothing new; protects everything after) | G11, G8 (bug half) | `build.rs` panics on the exact attribute shapes Phases 1–5 need; the SPI bugs are live correctness defects | S |
| **1** | `hx711`, `tm1637`, `dht11`/`dht22` | G1, G3, G6 | Five of the six device families the plan names, for one transport arm and three runtime impls | M |
| **2** | `ws2812b` (SPI), `apa102` | G8 (feature half), G10 (literal) | Reuses Phase 1's absent-pin machinery; small | S |
| **3** | (no new devices) | G13 | You are about to add manifests; migrating 9 now is cheaper than 14 later | S |
| **4** | `ds18b20` (single and multidrop) | G4 | Self-contained new subsystem, one family | M |
| **5** | thermistors, `mq-*`, potentiometers | G5 | Largest new subsystem (per-chip analog tables + Wi-Fi exclusivity) | L |
| **6** | RS-485/Modbus clusters, `bme280`/`bme680`/`adxl345` on either bus | G7, G9 | Needs Phase 1's transport shape to be settled first | M |
| **7** | (expressiveness) | G10 (rest), G12 | Nothing here unblocks a family; do it once the shape is stable | M |

**Progress:** Phase 0 and the Phase 1 machinery are landed: `bus "none"`, `PinOrRef` and
`open_drain` pins, `Device::Pins` emission (boot and deferred), and the runtime trait aliases and
impls. Phase 1.5 — the bundled `hx711`/`tm1637`/`dht22` manifests — is deferred until each crate's
real generic bounds and constructor arity are verified against the published crate, since a
manifest naming the wrong bounds fails in the generated project, not here. Phases 2–7 are not
started.

The single most important sequencing decision: **Phase 1 is one vertical slice, not four sub-milestones.** Each of `bus "none"`, `PinOrRef` pins, and the runtime traits is useless without the other two. Details in §4, Phase 1.

The second: **Phase 0 is not optional and not reorderable behind Phase 1.** Two of its four items are latent panics/bugs, not features.

---

## 1. Scope

**In scope:** the twelve gap items in `drivergaps.md` Part 1, phased as above.

**Deliberately out of scope for this plan:**

- The v1 `recipe` path for `<led>`/`<button>` (`espforge-elements/src/devices/pin.rs`). Phase 1 does not touch it — see §2.1.
- Any change to `format_version` numbers. Every change here is additive to `format_version 2`; existing manifests must load unchanged.
- New scheduler/executor features, new chips, or new `options` (beyond `requires_alloc` in Phase 7).
- Golden-project churn. Any phase that changes a bundled manifest's *behaviour* needs the change called out explicitly; cosmetic re-emission is a review failure.

---

## 2. Corrections to `drivergaps.md`

These change the work, so they are settled before Phase 0.

### 2.1 G2 ("pin-only recipes are `led`/`button`-locked") is overstated — Phase 1 skips it

That limitation is real for the v1 `recipe` child used by `<led>` and `<button>`: one pin, one bool. It is **not** the blocker for `hx711`/`tm1637`, which go through the v2 `pin` + `codegen` path.

Verified in `espforge-elements/src/devices/recipe.rs::lower_recipe` step 6: it already iterates `recipe.args`, filters `CtorArg::slot()`, and claims every pin slot in recipe order. `parse_pin` in `espforge-schema/src/driver.rs` already accepts `direction="input"|"output"|"bidirectional"`, `default_level`, `pull`, and `optional`. So an N-pin busless driver needs **no new recipe kind** — only (a) a pin attribute that accepts a bare GPIO number and (b) a transport arm that does not require a bus.

Consequence: `PinRecipeElement`, `PinDeviceKind`, and `parse_recipe` stay untouched. That removes roughly a fifth of the proposed Phase 1.

### 2.2 Do not add `BusTransport::Pins` — add a sibling `Device` variant

`BusTransport::bus()` returns `BusId` unconditionally and has three callers that assume a real bus:

- `BusDevice::emit` → `board.bus_mode(transport.bus())`
- `BusTransport::endpoint(asynchronous)` and the deferred-factory path, which need an `endpoint` type
- `lower_recipe`'s UART single-peer check and the "device on a bus" diagnostics

A `BusTransport::Pins` variant forces a sentinel `BusId` into all three, and `Board::bus_mode(BusId)` then either panics or silently consults the wrong bus. Use:

```rust
pub enum Device {
    Bus(BusDevice),
    Pin(PinDevice),
    /// A driver with no bus: its transport is the pins it claimed.
    Pins(PinsDevice),
}
```

`PinsDevice` carries `id`, `tag`, `driver: Driver`, and the claimed pins already live in `driver.pins`. `Device::as_bus()` keeps returning `Option<&BusDevice>`, so every existing caller stays total.

### 2.3 The Part 3 examples are the acceptance test, not the input

`drivergaps.md`'s `hx711` fragment uses `sck="4"` / `dout="5"` and types `espforge_runtime::OutputPin` / `InputPin`. Neither works today:

- every `pin` in a driver manifest is synthesized as `AttrKind::Ref { to: ["gpio"] }`, so `sck="4"` resolves as a `<gpio>` id and fails as "unknown gpio" — `drivergaps.md` §1.2 is a prerequisite, not polish;
- `espforge_runtime` exports no `OutputPin`/`InputPin` aliases; `Led` holds `esp_hal::gpio::Output<'static>` privately and `Bidirectional` holds `esp_hal::gpio::Flex<'static>`;
- `tm1637-gpio-driver` wants `InputPin + OutputPin` on the same line, which `Bidirectional` does not implement — only inherent `drive`/`release`/`level`.

So the examples are rewritten below (Phase 1.5) and are the acceptance criteria for Phase 1, not a source of already-true facts.

### 2.4 The `attr "delay" kind="choice"` + `map` pairing is load-bearing

The `hx711` and `tm1637` fragments declare `attr "delay" kind="choice" default="runtime"` and a matching `map "delay" { "runtime" { expr ".." } }`. That pairing is what makes `{delay}` a constructor argument resolved from `slots()`, and it must be preserved when those manifests are written for real. It also means `Delay` must satisfy the crate's delay bound on the **chosen** runtime — which is why the async `DelayNs` impl in Phase 1.4 is part of the same slice.

---

## 3. Verified baseline (what the code actually does today)

Recorded so each phase's "before" is unambiguous.

**Schema (`espforge-schema/src/driver.rs`)**

- `parse_driver` errors `driver "<tag>" has no bus` when `bus` is absent; `parse_bus` accepts exactly `i2c`, `spi`, `uart`.
- `add_bus_attrs` synthesizes `address` for I2C and `cs`/`mode`/`frequency` for SPI; `cs` is `AttrKind::PinOrRef` and **required**.
- `parse_pin` accepts `output`/`input`/`bidirectional`, `default_level`, `pull`, `optional`; direction defaults to `output`; `pull` on an output is an error.
- `parse_driver` then synthesizes each declared `pin` as `AttrDef { kind: AttrKind::Ref { to: ["gpio"] }, required: !optional }`.
- `parse_constructor` requires `{bus_device}` or `{spi_device}` first (or wrapping it in `args[0]`), then `args.remove(0)` consumes it; `path_text` rejects `PathArguments::AngleBracketed`, so no turbofish.
- `codegen { fallible #false }` and `runtime { awaits #false }` exist; `then { call ".." }` exists but always emits `.map_err(..)?`.

**Lowering (`espforge-elements/src/devices/recipe.rs::lower_recipe`)**

- resolves `bus="$id"` via `attrs::reference`, then requires `recipe.bus` to equal the peripheral's kind (`i2c`/`spi`/`uart`);
- `address`, `cs`, `init`, `mode`, `frequency` are consumed as connection facts, never constructor slots;
- pins are read with `attrs::opt_reference` and resolved through `cx.gpio(name)` — Ref only;
- `uart` refuses a second device on a port and refuses a driver whose runtime doesn't match the project's;
- final `match kind` is `i2c` / `spi` / `uart` with `_ => unreachable!`.

**Model / emit (`espforge-elements/src/model.rs`, `bus_emit.rs`)**

- `BusTransport { I2c, Spi, Uart }`; `SpiTransport.cs` is a non-optional `ClaimedPin`.
- `BusDevice::emit` resolves `board.bus_mode(transport.bus())`, builds the bus through `bus_emit::{i2c,spi,uart}_device`, and a deferred device without `endpoint` panics.
- `bus_emit::spi_device` takes `cs: u8` and always constructs `ConfiguredSpiDevice`/`ConfiguredAsyncSpiDevice` with an `Output::new(..)` CS line.

**Runtime (`espforge-runtime/src/lib.rs`, `hw/spi.rs`)**

- `Delay`: blocking `delay_ms` without `embassy`, async `delay_ms` with it, always-blocking `delay_ns`; implements only `embedded_hal::delay::DelayNs`. No `embedded_hal_async::delay::DelayNs`.
- `Bidirectional`: `Flex<'static>` with inherent `drive`/`release`/`level`/`is_high`/`is_low`. No `ErrorType`, `InputPin` or `OutputPin`.
- `ConfiguredSpiDevice::transaction`: `cs.set_level(asserted)` **then** `bus.configure(config)` — the glitch in G8.
- `ConfiguredAsyncSpiDevice::transaction`: asserts CS, calls `run()`, which does `self.bus.configure(..).await` (takes and releases the mutex), then `self.bus.inner.lock().await` separately — the race in G8.

**Attribute codegen (`espforge-elements/build.rs`, `src/attrs.rs`)**

- `field_type` has no arm for `(Hex, false, Some(_))`, `(PinOrRef, false, None)`, `(U32, false, None)`, `(Bool, false, None)` — all reach `panic!("unsupported attribute on ...")`.
- `attrs.rs` has `pin_or_ref` but no `opt_pin_or_ref`, `hex_or`, `opt_number`, `opt_flag`.

**Capability data (`espforge-elements/src/pin_caps.rs`)**

- `PinCaps { pins, input_only, flash, strapping }` for 11 chips. No ADC/DAC/touch channel maps.

**Tests that gate every phase**

- `espforge-elements/tests/manifests.rs` (bundled manifest corpus)
- `espforge/tests/golden_projects.rs`, `generated_projects_compile.rs`, `compile_fail.rs`, `offline_generation.rs`

---

## 4. Phases

### Phase 0 — Correctness and the attribute matrix

No new devices. Do this first because Phases 1–5 all write manifests whose attributes hit the `build.rs` panic, and because two of the four items are live defects.

**S.** No schema or format changes.

#### 0.1 `attrs.rs` helper set (G11, first half)

Add, following the existing `*_or` / `opt_*` naming:

| Helper | Signature | For |
|---|---|---|
| `opt_pin_or_ref` | `-> Result<Option<PinOrRef<'a>>, Issue<'a>>` | optional auxiliary pin |
| `hex_or` | `-> Result<Spanned<u8>, Issue<'a>>` | `address default="0x3C"` |
| `opt_number` | `-> Result<Option<Spanned<u32>>, Issue<'a>>` | optional `U32` |
| `opt_flag` | `-> Result<Option<Spanned<bool>>, Issue<'a>>` | optional `Bool` |

#### 0.2 `build.rs::field_type` matrix (G11, second half)

Add the four missing arms. Keep the existing panic as the catch-all — it is a useful assertion for a *future* unhandled kind, it just must not fire for a legal `(kind, presence, default)` triple. Add a table test asserting every `(AttrKind, bool, Option<&str>)` combination the schema corpus can produce has an arm; the test lives in `build.rs`'s own `#[cfg(test)]` or as a `manifests.rs` case, whichever is cheaper here.

#### 0.3 SPI chip-select ordering (G8, bug half)

In `ConfiguredSpiDevice::transaction`: move `self.bus.configure(self.config)?` **before** `self.cs.set_level(self.polarity.asserted())`. Same in `ConfiguredAsyncSpiDevice::transaction`: the configuration must be applied while CS is deasserted.

Invariant to document on both types: *CS is asserted only after the part's clock mode and rate are applied, and released before any other task may change them.*

#### 0.4 Async configure/lock atomicity (G8, race half)

`ConfiguredAsyncSpiDevice::run` currently configures under one lock acquisition and runs the transaction under another. Restructure so a single `self.bus.inner.lock().await` guard is held across `apply_config` + all operations + the `applied` cache update. This means `AsyncSpiBus::configure` can no longer be called from inside a device transaction with its own lock; expose a guard-taking form (e.g. `AsyncSpiBus::configure_locked(&mut Spi, config)`) and keep the public `configure` as a thin wrapper over it.

#### 0.5 Unify generated vs dynamic validation

Make `generated::validate` and `dynamic_attrs::validate` agree on `AttrKind::List` and on `range` checks. Today the two paths can disagree on a manifest that is validated twice (bundled vs. project-supplied); a Phase 1 manifest relying on `range` must not behave differently depending on which path saw it.

**Gate**

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test generated_projects_compile -- --ignored
```

**Acceptance:** `sht31.kdl` and `ssd1306.kdl` may now declare `address default="0x3C"` instead of `address allowed=..` with a required value; the manifest corpus passes; no golden project changes. The SPI ordering fix changes no generated code, only runtime behaviour — verify with the existing SPI device unit tests and one new ordering test that records `configure`/CS order on a fake bus.

---

### Phase 1 — Busless / GPIO-timed drivers

**Unlocks:** `hx711` (load cell), `tm1637` (7-segment), `dht11`/`dht22` (temperature/humidity).
**Gaps:** G1, G3, G6. **M.** One PR.

This is the only phase where the value-to-surface ratio is this good: one `Device` variant, one pin-attribute change, three runtime impls.

#### 1.1 Schema: `bus "none"` (G1)

In `espforge-schema/src/driver.rs`:

- `parse_bus` accepts `"none"`; reject any child node on it (`i2c`/`spi`/`uart` already reject unknown children — mirror that).
- `parse_driver`: when the bus is `none`, do **not** push the `bus` `AttrDef`. `id` remains required. The manifest must declare at least one `pin`, else the driver can construct nothing — error at load with a manifest-level message.
- `add_bus_attrs` adds nothing for `none`.
- `parse_constructor`: when the bus is `none`, skip the "bus first" requirement entirely — do not strip `args[0]`, do not require `{bus_device}`/`{spi_device}`. `{bus_device}`/`{spi_device}`/`{address}`/`{cs_pin}` are rejected as unknown-on-this-bus rather than silently accepted.
- `parse_runtime`: `endpoint` must not be required; today the *emitter* panics for a deferred device without one, so 1.3 below must make that conditional.

#### 1.2 Schema: pins become `PinOrRef`, plus open-drain (G3, first half)

- Synthesize `AttrKind::PinOrRef` for driver `pin` declarations rather than `Ref`. Keep `via="ref"` as an opt-in for a driver that needs strict `<gpio>` direction checking.
- Accept `direction="open_drain"` and map it to a new `PinDirection::OpenDrain`. Validation rules:
  - `default_level` is meaningful (`high` for an idle bus line) — allow it, unlike `input`;
  - `pull` is meaningful (external pull-up assumed; `pull="up"` enables the internal one) — allow it;
  - an open-drain pin is claimed as `PinUse::Bidirectional`.
- `PinDecl` keeps `optional`, which now reads through `opt_pin_or_ref`.

#### 1.3 Model: `Device::Pins` and its emission (G1/G3, second half)

Per §2.2:

- add `Device::Pins(PinsDevice)` where `PinsDevice { id, tag, driver }`, and `Device::as_bus()` stays `Option<&BusDevice>`;
- `PinsDevice::emit` mirrors the busless half of `BusDevice::emit`: build the constructor arguments from `driver.pins` + `driver.args` (no `{bus_device}` to fill), apply the same fallibility mapping, and wrap in `AsyncMutex` on an embassy board exactly as `PinDevice::emit` does;
- deferred init: allowed, with no `endpoint` — the factory stores the claimed raw peripherals (`esp_hal::peripherals::GPIO4<'static>` and friends) and constructs on `init()`. This is the piece that makes Phase 1.1's "no endpoint" requirement real.
- `lower_recipe` gains a `recipe.bus == "none"` arm that skips bus resolution, address handling, CS handling and `spi_config`, then returns the `Pins` device. Do **not** thread a sentinel `BusId` through the existing path.

#### 1.4 Runtime: traits and aliases (G6, G3 tail)

In `espforge-runtime/src/lib.rs`:

```rust
pub type OutputPin = esp_hal::gpio::Output<'static>;
pub type InputPin = esp_hal::gpio::Input<'static>;
```

plus, for the shared-line devices:

- `impl embedded_hal::digital::ErrorType for Bidirectional { type Error = core::convert::Infallible; }`
- `impl embedded_hal::digital::InputPin for Bidirectional` (`is_high`, `is_low`; both infallible)
- `impl embedded_hal::digital::OutputPin for Bidirectional` (`set_high`/`set_low` drive; a dedicated `release()` remains for open-drain high-Z, since `OutputPin::set_high` must not imply high-Z)
- **or**, if the semantics turn out to be indistinguishable in the target crates, a dedicated `OpenDrainPin` wrapper around `Flex<'static>` implementing both traits over `OutputOpenDrain`. Decide this by reading the two consuming crates' bounds before writing the impl; if `tm1637-gpio-driver` and `dht-sensor` both accept `InputPin + OutputPin`, the impl on `Bidirectional` is enough and `OpenDrainPin` is skipped.
- `Delay`:
  - keep `embedded_hal::delay::DelayNs`;
  - add `#[cfg(feature = "embassy")] impl embedded_hal_async::delay::DelayNs for Delay` using `embassy_time::Timer::after_nanos`;
  - add `Delay::delay_us_blocking(&self, us: u32)` as the explicit busy-wait for bit-banged slot timing inside an embassy task.

Re-export `esp_hal::gpio::{Input, Output, Flex}` through `espforge_runtime::gpio::*` so no manifest has to name `esp_hal::gpio::..` in a type string.

#### 1.5 Bundled manifests

Write the three (or four) manifests under `espforge-drivers/manifests/`, register them in `espforge-drivers/src/lib.rs::MANIFESTS`.

Acceptance shape — `tm1637`, the hardest of the three because both lines are open-drain:

```kdl
driver "tm1637" {
    format_version 2
    summary "..." 
    category "display"
    dependency "<crate>" { version "<exact>" }
    bus "none"
    pin "clk" direction="open_drain" default_level="high"
    pin "dio" direction="open_drain" default_level="high"
    codegen {
        fallible #false
        map "delay" { "runtime" { expr "espforge_runtime::Delay::new()" } }
        runtime "blocking" {
            type "<crate>::TM1637<'static, <pin type>, <pin type>, espforge_runtime::Delay>"
            constructor "<crate>::TM1637::new({clk}, {dio}, {delay})"
        }
    }
    attr "delay" kind="choice" default="runtime" { choices "runtime" }
    example "minimal" #"<tm1637 id="digits" clk="6" dio="7" />"#
}
```

Pin each crate's real generic bounds and constructor arity at write time — do not transcribe `drivergaps.md` Part 3 verbatim (§2.3). If a crate's `new` is infallible use `fallible #false`; if its post-construction setup returns `()`, that step needs Phase 7's `then { fallible #false }`, so either pick a crate without such a step or pull that one item forward.

#### 1.6 Tests

Fixtures in `espforge-elements/tests/manifests.rs` + `espforge/tests/generated_projects_compile.rs`:

1. A busless two-pin driver on a **blocking** board, `init` default (`boot`).
2. The same on an **embassy** board, both `boot` and `init="deferred"`.
3. An open-drain two-pin driver, asserting the generated field type and that the claimed pins are `PinUse::Bidirectional`.
4. Rejection cases: `bus "none"` with `{bus_device}` in the constructor; `bus "none"` with no `pin`; `direction="open_drain"` with `pull` on a chip where the pin is `input_only`; a busless driver bound with `bus="$i2c0"` (unknown/invalid attribute).
5. `espforge-runtime` trait tests (host): `Bidirectional` satisfies the `InputPin`/`OutputPin` bounds; `Delay` satisfies `embedded_hal_async::delay::DelayNs` under the `embassy` feature.

**Gate**

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test golden_projects -- --ignored
```

**Acceptance:** all three device families compile on blocking and embassy; golden churn limited to the new fixtures.

**Risks**

- *Open-drain semantics.* `esp-hal`'s `Flex` + `set_output_enable` is not the same as `OutputOpenDrain`; if a consuming crate calls `set_high()` expecting high-Z, `Bidirectional` actively drives the line high against the external pull-up (harmless, but not the assumption the crate makes), and `Bidirectional` driving low while another device drives the same line high is a bus short. Read the crate before choosing Phase 1.4's `Bidirectional`-vs-`OpenDrainPin` split.
- *Delay bounds.* A crate may require `DelayNs` (`&mut self`) where `map "delay"` passes by value. If so, add a `{delay}` form that passes `&mut` — but prefer picking a crate whose bound matches.
- *`Device` enum breadth.* `Device::Pins` adds a match arm in every exhaustive `match` over `Device`. Enumerate them (`grep -rn "Device::"`) before starting; this is mechanical but touches `lower.rs`, `resolve`, `raw_bus` emission and the emit tests' fixtures.

---

### Phase 2 — CS-less SPI and struct-literal constructors

**Unlocks:** `ws2812b` over SPI, `apa102`. **Gaps:** G8 (feature half), G10 (literal). **S.**

#### 2.1 Optional chip select

- `bus "spi" { cs_via "none" }`: `add_bus_attrs` omits `cs`; `lower_recipe` rejects a written `cs=` on such a device rather than ignoring it.
- `SpiTransport.cs` becomes `Option<ClaimedPin>`; every `spi.cs.pin().0` call site becomes an `Option` arm (emit, resolve, the SPI-CS-conflict check in `lower.rs`).
- `bus_emit::spi_device` returns a `SharedSpiBus`-style endpoint (implements `embedded_hal::spi::SpiBus` with `configure` before transfers, no CS toggling) when `cs` is `None`. Reuse Phase 0.3's ordering invariant: apply config, then transfer, with no CS to assert.

#### 2.2 `literal` constructors

- `runtime "blocking" { literal "crate::Type { field: {bus_device}, .. }" }` alongside `constructor`.
- Validate at load by substituting holes with sentinels and parsing as `syn::ExprStruct`; refuse closures, blocks, unknown holes, and `{placeholder}` in field-name or struct-path position.
- `Driver` gains an alternative constructor representation so emit can produce a struct literal instead of a call. Keep one `RustPath`-based path for the common case and add `CtorForm::{Call, Literal}` rather than replacing it.

**Gate:** workspace tests + `manifests.rs` + one compile fixture for a CS-less SPI device, and one `compile_fail` case for a literal with a placeholder in field position.

---

### Phase 3 — Endpoint aliases and manifest migration

**Gaps:** G13. No new devices. **S.** Do this before Phase 4 so five new manifests don't inherit the brittle strings.

- Export target-gated aliases from `espforge-runtime`:

```rust
pub type BlockingI2cEndpoint = embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>;
#[cfg(feature = "embassy")] pub type AsyncI2cEndpoint = AsyncI2cBus;
pub type BlockingSpiEndpoint = ConfiguredSpiDevice;
#[cfg(feature = "embassy")] pub type AsyncSpiEndpoint = ConfiguredAsyncSpiDevice;
pub type BlockingUartEndpoint = UartDevice<esp_hal::Blocking>;
#[cfg(feature = "embassy")] pub type AsyncUartEndpoint = UartDevice<esp_hal::Async>;
```

- Migrate `sht31.kdl`, `sht4x.kdl`, `ads111x.kdl`, `bme680.kdl`, `mhz19b.kdl` (and any Phase 1/2 manifest) to the aliases, in both `type` and `endpoint`.
- Because `type` strings also appear in generated code, confirm the alias expands identically — `generated_projects_compile` is the check. **Zero** golden churn is only achievable if the emitted type text is unchanged; if the alias text differs, this phase *will* churn goldens, so either accept and re-bless deliberately or keep `endpoint` alias-only and `type` as-is. Decide before migrating, and record the decision in the doc.

---

### Phase 4 — 1-Wire (ds18b20, single and multidrop)

**Gaps:** G4. **M.** Self-contained; after Phase 3 so the new bus names aliases from day one.

#### 4.1 Schema

- `<onewire id="ow0" pin="4" pull="internal|external" />` in `# peripherals` (`pull` defaults to `external`). Add to `espf.schema.kdl`'s peripherals section (whose summary currently reads "gpio, i2c, spi, uart" — update it).
- `bus "onewire" { address optional=#true }` in `parse_bus`; `add_bus_attrs` synthesizes a 64-bit ROM attribute. `AttrKind::Hex` is `u8`, so add `AttrKind::Hex64` (or `U64`) *with* its `attrs.rs` helper and its `build.rs` arm — expect this kind to be needed again.
- `{rom}` placeholder: `Option<u64>` when optional. Reject `{rom}` when the attribute is not declared.
- `check_onewire_roms` in `espforge-elements/src/lower.rs`, mirroring `check_i2c_addresses`: two devices on one `<onewire>` with the same explicit ROM is an error naming both.

#### 4.2 Runtime — `espforge-runtime/src/hw/onewire.rs`

- `OneWireBus` + `OneWireBusStorage`, following the `no_static_storage_in_runtime` contract: the storage static is declared in generated code (`static __OW0_STORAGE: OneWireBusStorage = OneWireBusStorage::new();`) exactly as `I2cBusStorage` is via `bus_emit::i2c_device`.
- A `Flex<'static>` line under `critical_section::Mutex<RefCell<..>>`.
- Slot operations inside `critical_section::with` using `esp_hal::delay::Delay`: `reset`, `write_bit`, `read_bit`, `write_byte`, `read_byte`, `search_rom`, CRC-8.
- `OneWireDevice` endpoint carrying `(OneWireBus, Option<Address>)` so a `ds18b20` driver can be adapted without blocking the executor during the 750 ms conversion (`ctx.delay.delay_ms(750).await` on embassy).
- `bus_emit::onewire_device`, mirroring `i2c_device`'s `device_once` + storage-static shape.

#### 4.3 Model / emit

- `BusTransport::OneWire(OneWireTransport { bus, address: Option<u64>, endpoint, async_endpoint })`. This one *does* carry a `BusId`, so the §2.2 concern does not apply.
- `Bus::OneWire` in `model.rs`, plus `bus_by_name`/`bus_mode` handling (a 1-Wire line is blocking-only; `bus_mode` must not resolve it to `Async`).

#### 4.4 Tests

- Two `ds18b20` on one `<onewire>`; duplicate-ROM rejection; pin-conflict rejection when a `<onewire>` and a device pin claim GPIO4.
- Single-probe manifestation with no `rom` attribute (search/first) on blocking and embassy.
- A host test for CRC-8 and for `search_rom` over a mocked line.

---

### Phase 5 — ADC and analog pin capability maps

**Gaps:** G5 (the ADC half; PWM/RMT/I2S/PCNT stay out). **L.** Largest new subsystem.

1. **`pin_caps.rs`**: add `adc1_pins: &'static [(u8, u8)]`, `adc2_pins`, `dac_pins`, `touch_pins` per chip, generated from `esp-metadata-generated` at the same commit the existing table was generated from, then committed. Extend `the_tables_are_self_consistent` to assert every listed pin is `< pins` and every channel number is in range.
2. **Schema**: `<adc id="adc1" unit="1" attenuation="11dB" />` in `# peripherals`; `bus "adc"` in `parse_bus`; the device synthesizes `pin` (checked against `adc1_pins`/`adc2_pins` for the target chip) and claims both the GPIO and the `(unit, channel)` slot in `ResourceGraph`.
3. **Exclusivity**: reject `ADC2` + `<wifi>` at lowering on ESP32/S2/S3/C3, with a diagnostic naming both elements (they share analog hardware and ADC2 fails at runtime while the radio is active).
4. **Runtime**: `hw/adc.rs` with `AdcChannelStorage` + `AdcChannel { read_raw() -> u16, read_mv() -> u16 }`, using eFuse calibration when available.
5. **Tests**: an ADC sensor fixture; the ADC2+Wi-Fi rejection; a per-chip channel-map consistency test.

Sequencing note: this phase is large enough that splitting it into 5a (tables + `<adc>` + raw reads) and 5b (calibration + `read_mv`) is reasonable. Do not start it before Phase 1 ships — the `pin_caps` work will want the same `PinOrRef`/`Device` shape and the same fixture harness.

---

### Phase 6 — Shared UART and multi-transport manifests

**Gaps:** G9, G7. **M.**

#### 6.1 Shared UART (`bus "uart" { shared #true }`)

- Default stays exclusive (the `lower_recipe` `"uart"` arm's single-peer refusal is correct for `mhz19b`).
- `shared #true` opts into a mutex-guarded `UartBus`/`UartBusStorage` mirroring `I2cBus`, with a per-device endpoint analogous to `CriticalSectionDevice`.
- The runtime-match refusal in the same arm (`espforge-runtime` compiles one `UartDevice` flavour per build) applies to a shared bus too; keep it.
- Pattern to unlock: Modbus RTU / RS-485 clusters with a shared `de`/direction-enable GPIO — which requires an optional `de` pin, i.e. Phase 1's optional `PinOrRef`.

#### 6.2 Multi-transport manifests (`transport "<kind>" { .. }`)

- A `driver` accepts either the existing top-level `bus` + `codegen`, or two-plus `transport "<kind>" { bus { .. } codegen { .. } }` blocks (`i2c`, `spi`, `uart`, `onewire`, `none`).
- `Element.attrs` synthesizes the union of transport-specific attributes as optional; `lower_recipe` resolves `bus="$id"` first, picks the matching `DeviceRecipe` for that bus kind, enforces that transport's required attributes, and **rejects** attributes belonging to another transport (`address=` on an SPI `<bme280>`).
- Per-transport `dependency` feature overrides (`transport "i2c" { dependency ".." { features "i2c" } }`) for crates like `bme280`/`mipidsi`.
- Migration: `bme280`, `bme680`, `adxl345`, `bmp280` become single manifests with two transports.

Risk: this is the phase that most stresses the recipe interpreter. It is deliberately after Phase 1 because `bus "none"` is itself a transport and must share the selection code.

---

### Phase 7 — Manifest expressiveness

**Gaps:** G10 (rest), G12. **M.** Nothing here unblocks a family; do it when a concrete manifest needs it.

1. `then { fallible #false }` and a rebinding form (`let #local = #local.into_..();`). Pull the `fallible #false` half forward if a Phase 1 crate needs it (Phase 1.5).
2. Turbofish in constructor paths: relax `path_text` to accept `PathArguments::AngleBracketed`, validated via `syn::Path`.
3. `requires_alloc #true` on `driver`/`dependency`, checked against `<options alloc="false">` / `defaults="false"` with a located `DEVICE_REQUIRES_ALLOC` diagnostic instead of a `cargo build` failure.
4. `alias "<tag>" "<tag>"` inside `driver`, registered in `Schema` with global uniqueness and emitted as `pub use` re-exports in `generated`.
5. Bounded recursive `compose`: replace the one-level cap in `check_compositions` with DFS cycle detection (reporting the full cycle), `MAX_COMPOSE_DEPTH = 4`, and deterministic `<parent>_<part>` sub-device ids.
6. `pin "..." direction="input" irq=#true` exposing `wait_for_edge`/`wait_for_low` through the generated `Context` without peripheral theft.

---

## 5. Cross-cutting decisions (settle once, apply everywhere)

| Decision | Choice | Rationale |
|---|---|---|
| Busless representation | `Device::Pins`, not `BusTransport::Pins` | `BusTransport::bus() -> BusId` has three callers that assume a real bus (§2.2) |
| Driver pin attribute kind | `PinOrRef` default, `via="ref"` opt-in | `cs` already behaves this way; `$gpio` remains available for direction checking |
| Open-drain | `direction="open_drain"`, claimed `PinUse::Bidirectional` | a fifth kind is not needed; `pin_caps` has no open-drain data |
| Endpoint type spelling | through `espforge_runtime` aliases | G13; decide the golden-churn question in Phase 3 before migrating |
| SPI config ordering | config before CS asserted, single lock across both | Phase 0.3/0.4 invariant, and a documented comment on both device types |
| Deferred devices | may omit `endpoint` when the transport is pins | today's panic in `BusDevice::emit` becomes a manifest error at load instead |

**New diagnostics codes** (naming convention follows `codes::DEVICE_INVALID` / `COMPONENT_UNKNOWN_GPIO`):

- `DEVICE_BUSLESS_WITH_BUS_ARG` — `{bus_device}` in a `bus "none"` constructor
- `DEVICE_BUSLESS_NO_PINS` — a `bus "none"` driver with no `pin`
- `PIN_OPEN_DRAIN_UNSUPPORTED` — open-drain on a pin the chip cannot drive
- `DEVICE_ONEWIRE_ROM_DUPLICATE`
- `DEVICE_ADC2_WIFI_CONFLICT`
- `DEVICE_REQUIRES_ALLOC`

---

## 6. Verification strategy

Run per phase, in this order, cheapest first:

```sh
cargo test -p espforge-elements --test manifests          # schema + lowering corpus
cargo test --workspace                                    # unit + host
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test golden_projects -- --ignored
cargo test -p espforge --test compile_fail -- --ignored
```

The target-gated runtime (`hw/spi.rs`, `hw/i2c.rs`) is invisible to host builds: `mod hw` is
`cfg(any(target_arch = "riscv32", target_arch = "xtensa"))`. `espforge-runtime` itself
carries neither a chip feature nor a console backend (a generated project supplies both),
so a direct check of a runtime change must name them:

```sh
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,embassy
```

**Known baseline defect:** `cargo test --workspace` fails on
`every_declared_cell_is_byte_identical_to_its_golden` because the committed goldens pin
`espforge-runtime = "0.9.0"` while the workspace is at `0.9.2`. It is pre-existing and
reproduces with no local changes; re-blessing belongs in the release commit
(`UPDATE_GOLDENS=1 cargo test -p espforge --test golden_projects`), not in a driver commit.
Judge each phase by "no *new* golden churn", not by a green workspace run.

Rules:

- **No unapproved golden churn.** A phase whose diff touches `espforge/tests/golden/` must say why in its PR description; cosmetic re-emission is a failure, not a re-bless.
- **Every new manifest gets an `example`** — `parse_driver` requires one, and `manifests.rs` exercises it.
- **Every rejection has a `compile_fail` case.** The diagnostics in §5 are as much a deliverable as the feature.
- **Runtime trait impls get host tests** where the impl is target-independent (`Bidirectional`'s `InputPin`/`OutputPin`, `Delay`'s async `DelayNs` bound), so a breakage doesn't need an ESP target to catch.

---

## 7. Risk register

| # | Risk | Phase | Mitigation |
|---|---|---|---|
| R1 | `Device::Pins` adds an exhaustive-match arm across `lower.rs`, `resolve`, `raw_bus` and test fixtures | 1 | Enumerate call sites with `grep -rn "Device::"` before starting; keep `as_bus()` returning `Option` |
| R2 | A third-party crate's delay bound takes `&mut D` where `{delay}` passes by value | 1, 2 | Read bounds first; prefer crates that take by value; add a `&mut` map form only if needed |
| R3 | Open-drain driving high fights an external pull-up or another device | 1 | Choose `OpenDrainPin` (high-Z on release) over `Bidirectional` (drive high) if the crate relies on release |
| R4 | `Hex64`/`U64` attribute kind ripples into `dynamic_attrs`, `build.rs`, and the JSON manifest fast-path | 4 | Implement the kind once in Phase 4 and reuse for any future 64-bit field |
| R5 | 1-Wire timing correctness under embassy (interrupt latency inside `critical_section`) | 4 | Allocate slots from `esp-hal` delay inside the critical section; add a scope-level protocol test with a mocked line |
| R6 | `esp-metadata-generated` version drift when generating ADC tables | 5 | Pin the same version the existing `pin_caps` table came from (0.5.3) and assert consistency in `the_tables_are_self_consistent` |
| R7 | Phase 3's aliases change emitted type text and churn every golden | 3 | Decide and record before migrating (Phase 3); if churn is unavoidable, do it as its own blessed commit |
| R8 | Multi-transport (Phase 6) reopens the recipe shape after Phases 1–5 built manifests on it | 6 | Keep `bus "none"` as a transport variant from the start, so Phase 6 generalizes rather than rewrites |

---

## 8. Gap map

| Gap | Description | Phase |
|---|---|---|
| G1 | Mandatory hardware bus | 1 |
| G2 | Pin-only recipes `led`/`button`-locked | **dropped** (§2.1) |
| G3 | Auxiliary pins lack `PinOrRef` + embedded-hal traits | 1 |
| G4 | No 1-Wire peripheral or ROM addressing | 4 |
| G5 | No ADC (PWM/RMT/I2S/PCNT deferred) | 5 |
| G6 | Asymmetric `Delay` traits | 1 |
| G7 | 1-to-1 single-transport lock-in | 6 |
| G8 | CS always required + clock glitch | 0 (bug), 2 (feature) |
| G9 | Exclusive UART ownership | 6 |
| G10 | `then` fallibility, ctor generics/literals | 2 (literal), 7 (rest) |
| G11 | `build.rs` attribute-matrix panics | 0 |
| G12 | No alloc/chip constraints, no aliases, no deep `compose` | 7 |
| G13 | Brittle `esp-hal` type strings | 3 |

---

## 9. Immediate next step

Phase 0, items 0.1 → 0.4, in a single PR. It is small, touches no device schema, unblocks every later phase's manifests, and fixes two live SPI defects on the way. Start with 0.2 (`build.rs`) because the very first Phase 1 manifest will hit it.

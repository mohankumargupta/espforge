# `planrmt.md`: Milestones for ESP32 RMT Support

**Status:** implementation plan
**Baseline:** `espforge` workspace `0.9.3`, branch `dev`, `esp-hal 1.2.2` (Cargo.lock), `esp-metadata-generated 0.5.3`
**Supersedes:** the phasing in `research/12-rmt.md` (same design intent, different order and seven corrected items — see §2)
**Purpose:** expose the RMT peripheral as `<rmt id="rmt0" />`, raw pulse channels (`<rmt_tx>` / `<rmt_rx>`), and a `bus "rmt"` transport for driver manifests, in milestones that each compile, test, and ship on their own.

---

## 0. The milestones, and why this order

| Milestone | Unlocks | Why here | Size |
|---|---|---|---|
| **M0** | (nothing user-visible; settles the API contract) | Every later file depends on the real `esp-hal 1.2.2` RMT signatures and on a gating mechanism that works across crates. Both are currently guessed in `12-rmt.md`. | S |
| **M1** | `<rmt id="rmt0" />` validated and accepted | Chip facts, diagnostic codes, schema, and `Bus::Rmt`. No pins, no channels, no runtime code. | S |
| **M2** | Raw IR-style TX/RX: `<rmt_tx>` / `<rmt_rx>` on blocking and Embassy | The first usable slice. One channel allocator, one runtime module, one pair of elements. Proves channel sharing on a single `RMT` block. | L |
| **M3** | `bus "rmt"` manifests + the WS2812/NeoPixel driver | Needs M2's allocator and `bus_emit` code to exist; the driver is the end-to-end acceptance test for the manifest path. | L |

**M0 is not optional.** The two facts it establishes — how `espforge-runtime` can gate RMT off on `esp32c2`/`esp32c61`, and what `Channel::transmit` actually consumes — change the code written in M2. Guessing either one produces a milestone that compiles on the host and fails on the target.

**The sequencing rule:** M2 is a vertical slice, not three parallel tasks. The runtime channel type, the `<rmt_tx>`/`<rmt_rx>` elements, the channel allocator, and the `bus_emit` builder are useless separately — no two of them can be demonstrated without the other two.

---

## 1. Scope

**In scope:** `<rmt>` in `# peripherals`; `<rmt_tx>` / `<rmt_rx>` raw channels; `bus "rmt"` for `format_version 2` manifests; blocking + Embassy runtimes; one bundled WS2812 driver.

**Out of scope for this plan (do not start these here):**

- DMA-driven RMT, PDM mode, or the RMT wake-up source.
- IR protocol decoding (NEC/Sony key maps). `<rmt_rx>` returns raw `PulseCode`s; decoding is application code.
- Bidirectional single-channel use (DShot telemetry). Needs TX→RX turnaround on one channel; treat as a follow-up once M3 lands.
- Chips whose RMT support is unverified (`esp32c5`, `esp32s31`, `esp32p4`). M1 decides them explicitly; an unverified chip gets no RMT rather than a wrong table.
- Deferred device init (`init="deferred"`) on RMT. See C7.
- Golden churn unrelated to new fixtures. Same rule as `plandriverfixes.md` §6: no unapproved re-bless.

---

## 2. Corrections to `research/12-rmt.md`

These change the work, so they are settled before M0.

### C1. `#[cfg(soc_has_rmt)]` cannot gate `espforge-runtime` — add an `rmt` cargo feature

`12-rmt.md` §3 Phase 3 says to gate `mod rmt;` with `#[cfg(soc_has_rmt)]`. That does not work: build-script cfgs are visible only inside the crate whose build script sets them (`esp-hal`), not to dependents. `espforge-runtime` declares no chip features, so `#[cfg(feature = "esp32c2")]` is not available either — the generated project supplies chip features through `esp-hal` feature unification.

Use the mechanism the runtime already has for `wifi` / `ota` / `http-client`:

- add `rmt = []` to `espforge-runtime/Cargo.toml`;
- gate `mod rmt;` and its re-exports with `#[cfg(feature = "rmt")]` inside `mod hw`;
- enable the feature in `espforge-emit/src/codegen/cargo.rs::runtime_dependency` when the board declares any RMT element.

`esp32c2`/`esp32c61` projects never declare `<rmt>` successfully (M1 rejects it with `E0530`), so they never enable the feature, so `hw/rmt.rs` is never compiled for them. That is the gate.

### C2. `<rmt>` alone emits no raw handle

Unlike `<i2c>`, `<spi>` and `<uart>`, the `Rmt` value is not directly useful: it is a bucket of unconfigured channel creators. A raw `device!(ctx, rmt0)` handle would expose exactly that bucket, and it cannot own a pin.

Decision:

- `<rmt>` with no `<rmt_tx>`/`<rmt_rx>` and no `bus "rmt"` device emits **nothing** into `Devices`. The element exists to validate the chip, claim `RMT`, and publish the bus.
- `emit_raw_buses` gets a `Bus::Rmt { .. } => continue` arm with a comment explaining why, so it does not grow a raw-bus emitter that cannot be built.
- raw pulse access is always a channel element (M2), never `<rmt>` itself.

Document this in `espf.schema.kdl` on `<rmt>`'s `description`, so "why did my `<rmt>` generate nothing?" is answered where the user looks.

### C3. Diagnostic codes: allocate a fresh RMT block, and fix the range arithmetic

`12-rmt.md` proposes `E0522..=E0526` and says to extend `the_slice_only_defines_the_codes_it_can_reach` from `501..=521` to cover them. Two problems:

1. `E0522` is already claimed as the first code by both `research/12-ledc.md` and `research/12-pcnt.md`. Whichever peripheral lands first must not silently own the number for all three.
2. Its channel codes `E0912`/`E0913` need the `901..=911` range extended to `901..=913`, not `901..=912`. (`E0910` is already a hole; leave it a hole — do not renumber.)

Allocation used by this plan:

| Code | Name | Meaning |
|---|---|---|
| `E0530` | `RMT_NOT_SUPPORTED` | `<rmt>` on a chip with no RMT (`esp32c2`, `esp32c61`) |
| `E0531` | `RMT_INVALID_ID` | id is not `rmt0` |
| `E0532` | `RMT_INVALID_FREQUENCY` | `frequency=` is not a frequency, or is 0 |
| `E0912` | `RMT_CHANNEL_UNAVAILABLE` | every TX (or RX) channel is already claimed |
| `E0913` | `RMT_CHANNEL_CONFLICT` | requested channel is wrong-direction for the chip, already claimed, or out of range |

Reuse, do not duplicate: `E0704 CONFLICT_PERIPHERAL` (second `<rmt>`, via `claim_peripheral("RMT", ..)`), `E0701 CONFLICT_PIN` (pin clash with another element), `E0518 GPIO_NOT_OUTPUT_CAPABLE` (TX on an input-only GPIO).

And update the range test in `espforge-diagnostics/src/codes.rs` to `501..=532 | ... | 901..=913`.

### C4. On `esp32` and `esp32s2`, TX and RX are one pool of channels

`12-rmt.md` models `RmtCaps { tx_channels, rx_channels }` as if they were disjoint. On `esp32` (8 channels) and `esp32s2` (4 channels) every channel can be either direction, so two devices could claim channel 0 once as TX and once as RX and both compile.

`RmtCaps` must still expose both lists (direction checks need them), but the allocator must claim the **channel index**, not `(index, direction)`:

```rust
// espforge-elements/src/model.rs
cx.claim_peripheral(format!("RMT.channel{channel}"), owner, span)?;
```

On `esp32s3`/`c3`/`c6`/`h2` the lists are disjoint, so the same rule is sufficient and correct.

### C5. Do not design `RmtTxChannel` around a consuming `transmit` until M0 says so

`12-rmt.md` asserts `TxChannel::transmit(self, data)` consumes the channel and `.wait()` returns the channel back, and prescribes an `Option<Channel>` dance. In `esp-hal 1.2.2` this may be a borrowing call that returns a transaction object. M0 records the real signature; M2 implements the shape it dictates.

If the call consumes `self`: `RmtTxChannel { channel: Option<Channel<..>> }`, `transmit(&mut self, ..)` takes the channel, waits, puts it back (returning `RmtError` and a reconstructed value on error is not required — the channel is still usable and the error is reported).

If the call borrows `&mut self`: no `Option`, no dance. Do not copy the `Option` workaround "just in case".

### C6. Name the endpoint types from day one; do not repeat Phase 3 of `plandriverfixes.md`

There are no existing RMT manifests, so there is nothing to migrate. Export the four aliases from `espforge_runtime` in M2 and use them in every manifest and element from the start:

```rust
pub type BlockingRmtTxEndpoint = RmtTxChannel<esp_hal::Blocking>;
#[cfg(feature = "embassy")] pub type AsyncRmtTxEndpoint = RmtTxChannel<esp_hal::Async>;
pub type BlockingRmtRxEndpoint = RmtRxChannel<esp_hal::Blocking>;
#[cfg(feature = "embassy")] pub type AsyncRmtRxEndpoint = RmtRxChannel<esp_hal::Async>;
```

No manifest may spell `esp_hal::rmt::..`. If M0 finds the channel index in the type (`Channel<'d, Dm, Tx, N>`), the aliases cannot be written this way — M0 must then choose `.degrade()`/`AnyTxChannel` or a small enum wrapper, and record which.

### C7. Refuse `init="deferred"` on RMT devices in v1, explicitly

A deferred factory must store the channel it was built with. Channels come from moving fields out of the single `Rmt` local inside `Devices::new`; a deferred path would have to store a half-consumed `Rmt` or a channel plus its pin, and none of that is exercised by M2/M3's use cases.

Refuse it at lowering with a located error (`E0907 DEVICE_INVALID`, label: "an RMT device is built at boot; remove `init=\"deferred\"`"). If a later milestone wants deferred strips, that is its own design.

### C8. The WS2812 driver needs no third-party crate, and the LED count is not a const generic (yet)

`12-rmt.md`'s example names `espforge_drivers::Ws2812Rmt<{leds}>` and depends on `smart-leds`. Two avoidable risks:

- `{leds}` filling a const-generic hole in `type` is not exercised by any current manifest. Verify it in M3; if it does not work, take the pixel count as a `write(&mut self, &[[u8; 3]])` argument instead of a type parameter. Prefer that anyway for v1 — it removes the whole question.
- `smart-leds` is only a trait vocabulary. The encoder is ~20 lines; a dependency is not earned.

v1 driver: `espforge_drivers::Ws2812Rmt` / `AsyncWs2812Rmt` wrapping a runtime TX endpoint, with `write(&mut self, pixels: &[[u8; 3]]) -> Result<(), RmtError>` (GRB order, reset pulse included).

---

## 3. Verified baseline (what the code actually does today)

Recorded so each milestone's "before" is unambiguous.

**Chip facts (`espforge-elements/src/chips.rs`)**

- `has_wifi` reads `esp-metadata`'s `soc_has_wifi` (`chip.contains("soc_has_wifi")`); `i2c_instances` / `uart_instances` are hand-kept matches.
- No RMT helper exists. `NOT_YET_SUPPORTED = ["esp32p4"]`; `esp32c5`, `esp32s31` are offered.

**Diagnostics (`espforge-diagnostics/src/codes.rs`)**

- Peripherals: `E0501..=E0521` (with `E0521` = `UART_INVALID_BAUD`). Devices: `E0901..=E0911` with `E0910` unused. Conflicts: `E0701..=E0709`, including `E0704` = `CONFLICT_PERIPHERAL`.
- `the_slice_only_defines_the_codes_it_can_reach` accepts `501..=521` and `901..=911`.

**Schema (`espforge-schema/src/driver.rs`, `espf.schema.kdl`)**

- `parse_bus` accepts `i2c | spi | uart | none`, takes an optional `transport_kind` for the existing `transport "<kind>" { .. }` blocks, and fills `BusDecl { kind, address, address_default, address_allowed, spi_config, mode, frequency, cs_polarity, span }`.
- `section "peripherals"` summary reads "gpio, i2c, spi, uart".
- `<rmt>`, `<rmt_tx>`, `<rmt_rx>` do not exist.

**Model (`espforge-elements/src/model.rs`)**

- `OwnerKind` = `Gpio | Pin | I2c | Spi | Uart | Device | Wifi`; `tag()` is exhaustive.
- `Bus` = `I2c | Spi | Uart`; `Bus::id()` is exhaustive.
- `BusTransport` = `I2c | Spi | Uart`, with `bus()` and `endpoint(asynchronous)` matching all three.
- `Device` = `Pin | Bus | Pins`; `as_pin`, `as_bus`, `driver` and `Lowered::emit` match all three.
- `BusMode` is per-bus; `Board::bus_mode(BusId)` resolves it; `lower.rs::check_bus_modes` refuses a blocking-only + async-only pair on one bus (`E0909`).

**Emission (`espforge-elements/src/bus_emit.rs`, `raw_bus.rs`, `element.rs`)**

- Helpers: `gpio(pin)`, `peripheral_access("RMT")`, `rate(khz)`, `int(u32)`; `cx.device_once(key, stmt)` appends a statement once and reports whether it inserted (used to declare storage statics).
- `i2c_device` / `uart_device` / `spi_device` build their controller once and return the device endpoint.
- `emit_raw_buses` iterates every bus with no `as_bus()` device and matches `Bus` exhaustively (`I2c`/`Spi`/`Uart`).
- `espforge-emit/src/codegen/rust.rs::no_static_storage_in_runtime` (line 744) forbids `static` items inside `espforge-runtime/src`.

**Runtime (`espforge-runtime`)**

- `mod hw` is `cfg(any(target_arch = "riscv32", target_arch = "xtensa"))`; modules `i2c`, `spi`, `uart` (+ `ota`, `wifi` feature-gated). No chip feature, no `rmt`.
- Features today: `embassy`, `wifi`, `http-client`, `https-client`, `ws-client`, `wss-client`, `ota`, `ntp`.
- `esp-hal` is `{ version = "1", features = ["unstable"] }`, target-gated; `Cargo.lock` pins `1.2.2`.

**Cargo emission (`espforge-emit/src/codegen/cargo.rs`)**

- `runtime_dependency(source, embassy, wifi, ota, board)` builds the feature list: `embassy`, then `wifi`, `ota`, then `board.service_features()`. This is where `rmt` joins, not in a hand-written template.
- `drivers_dependency` uses `board.driver_features()`.

**Tests that gate every milestone**

- `espforge-elements/tests/manifests.rs`
- `espforge/tests/generated_projects_compile.rs`, `golden_projects.rs`, `compile_fail.rs`, `offline_generation.rs`
- Examples live at `espforge-examples/src/examples/04.Communication/<name>/app/rust/app.rs`.

---

## 4. Milestones

### M0 — API and gating contract

**S.** No schema, no diagnostics, no generated-code change.

Deliverables:

1. `espforge-runtime/Cargo.toml`: add `rmt = []`.
2. `espforge-runtime/src/lib.rs`: `#[cfg(feature = "rmt")] mod rmt;` inside `mod hw`, with `pub use rmt::*;`.
3. `espforge-runtime/src/hw/rmt.rs`: a skeleton whose doc comment records, from `esp-hal 1.2.2`'s actual source:

   | Question | Answer recorded |
   |---|---|
   | `Rmt::new` signature | takes `peripherals.RMT` + `Rate`; returns `Result<Rmt<'static, Blocking>, Error>`? |
   | `into_async` | on `Rmt` or on each channel? |
   | `ChannelCreator::configure_tx` | takes `&TxChannelConfig`? returns `Result<Channel<..>, Error>`? is the index in the type? |
   | `with_pin` | separate builder method or config field? |
   | `PulseCode` | `new(level1, len1, level2, len2)` + `end_marker()`; half-length bit width |
   | TX ownership | `transmit(self)` / `transmit(&mut self)`; what `wait()` returns |
   | RX | `receive(&mut [PulseCode]) -> Result<usize, Error>`? |
   | Error type | `esp_hal::rmt::Error` — alias it as `RmtError` |
   | `DriverMode` | where `Blocking`/`Async` live and how the type parameter is spelled |
   | Carrier | `TxChannelConfig::with_carrier_modulation` shape and duty argument bounds |

4. One throwaway `#[cfg(feature = "rmt")] pub fn _probe(...)` (or a doc-test-free compile check) that instantiates the TX type, so the target checks actually type-check the API rather than an empty module.

**Gate**

```sh
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,rmt
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,rmt,embassy
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c2,esp-println/esp32c2,esp-println/uart
```

**Acceptance:** the three checks pass; the API table is committed in the module docs. `--features ...,rmt` on `esp32c2` is expected to fail — verify it fails with "no `rmt` in `esp_hal`" rather than something confusing, and record that in the doc. Do not make it a test.

---

### M1 — Chip facts, diagnostics, `<rmt>`

**S.** No runtime module, no pins, no channels.

#### 1.1 `chips.rs`

```rust
pub fn has_rmt(name: &str) -> bool;                 // reads soc_has_rmt from esp-metadata
pub struct RmtCaps {
    pub tx_channels: &'static [u8],
    pub rx_channels: &'static [u8],                 // overlaps tx on esp32/esp32s2 (C4)
    pub base_mhz: u32,
}
pub fn rmt_caps(chip: &str) -> Option<RmtCaps>;
```

Table (verify `has_rmt` against `esp-metadata` first; these are the expected answers):

| Chip | TX | RX | base |
|---|---|---|---|
| `esp32` | 0..=7 | 0..=7 | 80 MHz |
| `esp32s2` | 0..=3 | 0..=3 | 80 MHz |
| `esp32s3` | 0..=3 | 4..=7 | 80 MHz |
| `esp32c3`, `esp32c6` | 0..=1 | 2..=3 | 80 MHz |
| `esp32h2` | 0..=1 | 2..=3 | **32 MHz** |
| `esp32c2`, `esp32c61` | `None` | `None` | — |
| `esp32c5`, `esp32s31`, `esp32p4` | `None` until verified | | |

Tests: `has_rmt("esp32c2") == false`, `has_rmt("esp32c3") == true`, `rmt_caps("esp32h2").unwrap().base_mhz == 32`, and a table-consistency test that every listed channel is `< 8`, every TX/RX index is in range, and **every offered chip has a deliberate answer** (an exhaustive match with explicit `None` arms, so a new chip cannot silently inherit support).

#### 1.2 Diagnostics

Add the five codes from C3 and update the range test (`501..=532`, `901..=913`).

#### 1.3 Schema

```kdl
element "rmt" section="peripherals" {
    summary "RMT pulse-train peripheral"
    description "Declares the chip's RMT block. It builds nothing on its own: add <rmt_tx>/<rmt_rx> channels, or bind a bus=\"rmt\" driver."
    attr "id" kind="ident" required=#true {
        help "rmt0: the chip has one RMT block."
    }
    attr "frequency" kind="freq" { help "Source clock; defaults to 80MHz, or 32MHz on esp32h2." }
    example "minimal" #"<rmt id="rmt0" />"#
    example "32MHz source" #"<rmt id="rmt0" frequency="32MHz" />"#
}
```

Update `section "peripherals"`'s summary to include `rmt`.

#### 1.4 Element

`espforge-elements/src/peripherals/rmt.rs`, registered with `#[distributed_slice(ELEMENTS)]`:

- refuse a chip without RMT (`E0530`) before anything else;
- accept only `rmt0` (`E0531`) — there is one block, so an id like `rmt1` would be a lie;
- `frequency` through the existing `peripherals::parse_khz` (`E0532`), defaulting to `rmt_caps(chip).base_mhz * 1000`;
- `cx.claim_peripheral("RMT", owner, span)?` → a second `<rmt>` is `E0704`;
- `cx.add_bus(Bus::Rmt { id, peripheral: "RMT".into(), khz })`.

#### 1.5 Model

- `OwnerKind::Rmt` (`tag() -> "rmt"`).
- `Bus::Rmt { id, peripheral, khz }`, plus its arm in `Bus::id()`.
- `raw_bus.rs`: `Bus::Rmt { .. } => continue` with the C2 comment.

**Gate**

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test generated_projects_compile -- --ignored
```

**Acceptance:** a project with `<rmt id="rmt0" />` on `esp32c3` compiles and emits no `Devices` field; `esp32h2`'s default is 32 MHz; `esp32c2` is refused with `E0530`; a second `<rmt>` is `E0704`; `frequency="0Hz"` is `E0532`; no existing golden project changes.

---

### M2 — Raw channels: runtime, `<rmt_tx>` / `<rmt_rx>`, emission

**L.** The first milestone a user can see working.

#### 2.1 Runtime — `espforge-runtime/src/hw/rmt.rs`

- `pub use esp_hal::rmt::{PulseCode, RxChannelConfig, TxChannelConfig};`
- `pub type RmtError = esp_hal::rmt::Error;`
- `RmtTxChannel<Dm>` / `RmtRxChannel<Dm>` holding `esp_hal::rmt::Channel<'static, Dm, Tx/Rx>` (shape per C5/M0).
- Blocking:
  - `transmit(&mut self, data: &[PulseCode]) -> Result<(), RmtError>`
  - `receive(&mut self, buf: &mut [PulseCode]) -> Result<usize, RmtError>`
- `#[cfg(feature = "embassy")]` async equivalents with the same names (a `Dm`-parameterised type means only one impl block is legal per `Dm`, so the names can match).
- `RmtTxChannel::new(channel) -> Self` / `RmtRxChannel::new(channel) -> Self` (the element passes the configured channel; the type is the alias endpoint).
- Aliases from C6.
- **No `static` anywhere** (`no_static_storage_in_runtime`), and no buffer storage: RX buffers are the caller's.

#### 2.2 Model

```rust
pub struct RmtTransport {
    pub bus: BusId,
    pub channel: u8,
    pub direction: RmtDirection,       // Tx | Rx
    pub pin: ClaimedPin,
    pub clk_divider: u8,
    pub idle_level: Level,             // Tx only
    pub carrier: Option<Carrier>,      // Tx only: hz + duty percent
    pub endpoint: Option<RustType>,
    pub async_endpoint: Option<RustType>,
}

pub struct RmtChannelDevice {
    pub id: Ident,
    pub tag: String,                   // "rmt_tx" | "rmt_rx"
    pub transport: RmtTransport,
}

pub enum Device { Pin(..), Bus(..), Pins(..), Rmt(RmtChannelDevice) }
```

Arm every exhaustive match: `Device::id`, `tag`, `as_pin`, `as_bus`, `driver`, `Lowered::emit`, `overlay_attr`. Grep `Device::` before starting; this is `plandriverfixes.md`'s R1 pattern.

#### 2.3 Channel allocator (one helper, two callers)

In `espforge-elements/src/model.rs` or a small `rmt.rs` helper:

```rust
pub(crate) fn allocate_rmt_channel(
    cx: &mut LowerCx<'_>,
    caps: &RmtCaps,
    direction: RmtDirection,
    requested: Option<u8>,
    owner: Owner,
    span: Range<usize>,
) -> Result<u8, Reported>;
```

- `requested`: must be in the direction's list (`E0913` otherwise — on `esp32c3`, `channel="2"` is RX-only, so TX is `E0913`);
- claim `format!("RMT.channel{n}")` through `cx.claim_peripheral` (`E0913` on clash; transactional rollback frees it if the element later fails);
- no request: lowest unclaimed index in the direction's list, else `E0912`.

Both `<rmt_tx>`/`<rmt_rx>` (this milestone) and `lower_recipe` (M3) call this. Do not write the allocation twice.

#### 2.4 Elements — `espforge-elements/src/devices/rmt.rs`

Schema:

```kdl
element "rmt_tx" section="devices" {
    summary "Raw RMT transmit channel"
    attr "id" kind="ident" required=#true
    attr "bus" kind="ref" to="rmt" required=#true
    attr "pin" kind="pin" required=#true
    attr "channel" kind="u32" { help "0-based; auto-allocated when omitted." }
    attr "clk_divider" kind="u32" default="1" { help "1..=255." }
    attr "idle_level" kind="choice" default="low" { choices "low" "high" }
    attr "carrier_hz" kind="u32"
    attr "duty" kind="u32" default="50" { help "Carrier duty, percent." }
    example "minimal" #"<rmt_tx id="ir_tx" bus="$rmt0" pin="4" />"#
}
```

`<rmt_rx>` is the same minus `idle_level`, `carrier_hz`, `duty`; reject those three at lowering rather than ignoring them.

Lowering per element:

1. resolve `bus="$rmt0"` via `attrs::reference`, require `Bus::Rmt`;
2. claim `pin` as `PinUse::Output` (TX) / `PinUse::Input` (RX), through the same `cx.claim_pin` path that yields `E0701`/`E0518`;
3. `allocate_rmt_channel`;
4. validate `clk_divider` in `1..=255` and `duty` in `1..=100`;
5. push `Device::Rmt` with endpoint alias `BlockingRmtTxEndpoint` / `AsyncRmtTxEndpoint` (and RX mirror).

#### 2.5 Emission — `bus_emit::rmt_channel`

- `cx.device_once("rmt0_bus", quote! { let #handle = esp_hal::rmt::Rmt::new(#peripheral, #rate).expect("RMT configuration failed")[.into_async()]; })` — `.into_async()` iff `board.bus_mode(bus) == BusMode::Async`.
- Move the channel: `let mut channel = #handle.channel#n;`, then configure per M0's real API, then `.with_pin(#gpio)` (or the config's pin field).
- Return the constructed `espforge_runtime::RmtTxChannel::new(channel)` (or its alias) as the `Devices` field, wrapped in `AsyncMutex` on an Embassy board exactly as `PinsDevice::emit` does.

Two invariants to write as comments in the module:

- **`#handle` is a local, never an owned `Devices` field.** Channels are moved out of it field by field; partial moves are legal only because nothing touches `#handle` as a whole afterwards. Do not `Debug`-print it, borrow it wholesale, or return it.
- **`device_once` must run before the first channel move**, and it does: every channel builder calls it first.

#### 2.6 Tests

`espforge-elements/tests/manifests.rs` + `compile_fail`:

1. Two TX channels on one `<rmt>` on `esp32s3` (`channel="0"` and auto-allocated `1`) both appear, one `Rmt::new`.
2. Auto-allocation skips a manually claimed channel.
3. `esp32c3`: TX on `channel="2"` → `E0913`.
4. Duplicate `channel="0"` → `E0913`, and rollback leaves `RMT.channel0` claimable by the next element.
5. `esp32s3` with five TX channels → `E0912`.
6. TX pin shared with an `<led>` → `E0701`; TX on `GPIO34` (`esp32`, input-only) → `E0518`.
7. `<rmt_rx carrier_hz="38kHz">` → manifest/lowering error naming the attribute.
8. `init="deferred"` on `<rmt_tx>` → `E0907` (C7).

Examples: `04.Communication/rmt_tx` (blocking, NEC-style pulse train) and `04.Communication/rmt_tx_async`. An RX example is welcome but optional in this milestone.

**Gate**

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test compile_fail -- --ignored
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test golden_projects -- --ignored
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,rmt,embassy
```

**Acceptance:** the two examples generate and compile for `esp32c3` and `esp32s3`; two channels share one `RMT`; every rejection has a `compile_fail` case; no pre-existing golden project changes (new fixtures only).

---

### M3 — `bus "rmt"` manifests and the WS2812 driver

**L.** Ends with a strip that actually lights up.

#### 3.1 Schema — `parse_bus`

- accept `"rmt"` in the kind check (and in `transport "rmt" { .. }`, since `parse_bus` already carries `transport_kind`);
- children: `direction "tx" | "rx"` (required), `clk_divider <1..=255>` (default 1), `idle_level "low" | "high"` (TX only, default low), `carrier_hz <u32>` + `duty <1..=100>` (TX only), `idle_output` if M0 shows the API wants it;
- store as `BusDecl.rmt: Option<RmtBusConfig>`; reject unknown children like the other kinds;
- `add_bus_attrs` / `add_union_bus_attrs` synthesize `pin` (`AttrKind::PinOrRef`, required) and `channel` (`AttrKind::U32`, optional);
- `parse_constructor`: `{bus_device}` allowed; `{address}` and `{cs_pin}` rejected as unknown on this bus;
- a `direction`-less `bus "rmt"` block is a load error, not a default: a driver that does not say TX or RX cannot allocate a channel.

#### 3.2 Lowering — `lower_recipe`

- `kind == "rmt"` arm: resolve the bus, require `Bus::Rmt`; claim `pin` (TX `Output`, RX `Input`); `allocate_rmt_channel`; build `BusTransport::Rmt`; copy `clk_divider`/`idle_level`/`carrier` from `RmtBusConfig`;
- refuse `init="deferred"` (C7);
- `address`/`cs`/`spi_config` must not be reachable on this arm.

#### 3.3 Model / emit

- `BusTransport::Rmt(RmtTransport)`, plus arms in `bus()` and `endpoint(asynchronous)`;
- `BusDevice::emit` needs no change: it already resolves `bus_mode(transport.bus())` and `transport.endpoint(..)`;
- `bus_emit::rmt_device(cx, board, &RmtTransport)` shares the `device_once` + channel-move code with M2's `rmt_channel` — extract one function, do not copy;
- `check_bus_modes` applies unchanged. `Rmt::new` has one `DriverMode` for the whole block, so a blocking-only driver plus an async-only driver on one `<rmt>` is `E0909`, and every driver on `<rmt>` must declare the mode the board resolved.

#### 3.4 First driver — `espforge-drivers`

- `espforge-drivers/src/ws2812.rs`, target-gated like `scd4x.rs`, features `ws2812-rmt` (blocking) and `ws2812-rmt-async`;
- `Ws2812Rmt { channel: BlockingRmtTxEndpoint }` with `write(&mut self, pixels: &[[u8; 3]]) -> Result<(), RmtError>`;
- encode from the **actual base clock**: at 80 MHz / divider 1 emit T0H 32, T0L 68, T1H 64, T1L 36 ticks, then a ≥50 µs low reset; on `esp32h2` (32 MHz) recompute from 40 ticks/bit. Never hardcode 80 MHz (this is `12-rmt.md` §1 point 3, made concrete);
- manifest `espforge-drivers/manifests/ws2812.kdl`:

```kdl
driver "ws2812_rmt" {
    format_version 2
    summary "WS2812 / NeoPixel addressable RGB strip over RMT"
    category "display"
    bus "rmt" {
        direction "tx"
        clk_divider 1
    }
    codegen {
        feature "ws2812-rmt"
        runtime "blocking" {
            type "espforge_drivers::Ws2812Rmt"
            constructor "espforge_drivers::Ws2812Rmt::new({bus_device})"
            endpoint "espforge_runtime::BlockingRmtTxEndpoint"
        }
        runtime "async" {
            feature "ws2812-rmt-async"
            type "espforge_drivers::AsyncWs2812Rmt"
            constructor "espforge_drivers::AsyncWs2812Rmt::new({bus_device})"
            endpoint "espforge_runtime::AsyncRmtTxEndpoint"
        }
    }
    example "minimal" #"<ws2812_rmt id="strip" bus="$rmt0" pin="8" />"#
}
```

Register it in `espforge-drivers/src/lib.rs::MANIFESTS`; add `06.Displays/ws2812` (blocking) and `06.Displays/ws2812_async`.

#### 3.5 Tests

1. A manifest with `bus "rmt" { direction "tx" clk_divider 1 }` lowers to `BusTransport::Rmt` with the auto channel.
2. `direction "rx"` + `carrier_hz` → load error naming `carrier_hz`.
3. Missing `direction` → load error.
4. `direction "tx"` on `esp32c3` with `channel="2"` → `E0913`.
5. `address="0x3C"` on a `bus "rmt"` device → unknown-attribute error, not silently ignored.
6. WS2812 manifest: lowering, `drivers_dependency` feature, and generated-project compile for `esp32c3` blocking and `esp32s3` embassy.
7. A blocking-only + async-only pair on one `<rmt>` → `E0909`.
8. Pin conflict between the strip's `pin` and an `<led>` → `E0701`.

**Gate**

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test compile_fail -- --ignored
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test golden_projects -- --ignored
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,rmt,embassy
```

**Acceptance:** the WS2812 example compiles on both runtimes; the feature appears in the generated `Cargo.toml`; goldens change only for the new fixtures (re-bless deliberately, in its own commit).

---

## 5. Cross-cutting decisions (settle once, apply everywhere)

| Decision | Choice | Rationale |
|---|---|---|
| Gating RMT in the runtime | `rmt` cargo feature, enabled by `runtime_dependency` | Build-script cfgs do not cross crates (C1) |
| `<rmt>` with no channels | Emits nothing | `Rmt` is a bucket of unconfigured channels (C2) |
| Raw pulse access | `<rmt_tx>` / `<rmt_rx>` elements, `Device::Rmt` | No fake `Driver` record, no second `BusTransport` path |
| Channel ownership | `cx.claim_peripheral("RMT.channel{n}")` by index | TX/RX pools overlap on esp32/s2 (C4) |
| Channel allocation | one helper, called by elements and `lower_recipe` | Two implementations will diverge |
| Endpoint naming | `espforge_runtime::{Blocking,Async}Rmt{Tx,Rx}Endpoint` | No legacy RMT manifests to migrate (C6) |
| One `Rmt` per project | `<rmt>` claims `"RMT"`; second is `E0704` | One `Rmt::new` per build |
| Driver mode | `board.bus_mode(bus)` → `.into_async()` or not; all channels share it | `esp-hal`'s `DriverMode` is per `Rmt` |
| Deferred init | refused on RMT (`E0907`) | Channel comes from a partial move out of a local (C7) |
| WS2812 LED count | `write(&[[u8; 3]])` argument, no const generic | Avoids an unverified `{leds}` type hole (C8) |
| WS2812 timing | computed from `rmt_caps(chip).base_mhz` | `esp32h2` is 32 MHz, not 80 |

**New diagnostics:** `E0530 RMT_NOT_SUPPORTED`, `E0531 RMT_INVALID_ID`, `E0532 RMT_INVALID_FREQUENCY`, `E0912 RMT_CHANNEL_UNAVAILABLE`, `E0913 RMT_CHANNEL_CONFLICT` (plus the reused `E0704`, `E0701`, `E0518`, `E0909`).

---

## 6. Verification strategy

Run per milestone, cheapest first:

```sh
cargo test -p espforge-elements --test manifests        # schema + lowering corpus
cargo test -p espforge-diagnostics                      # code table and ranges
cargo test --workspace                                  # unit + host
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test golden_projects -- --ignored
cargo test -p espforge --test compile_fail -- --ignored
```

The RMT runtime is invisible to host builds (`mod hw` is target-gated), so every runtime change also needs an explicit target check. `espforge-runtime` carries neither a chip feature nor a console backend, so name both:

```sh
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,rmt
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,rmt,embassy
# The gate itself: no RMT feature, no RMT module, no esp_hal::rmt reference.
cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
  --features esp-hal/esp32c2,esp-println/esp32c2,esp-println/uart
```

Rules:

- **No unapproved golden churn.** A milestone whose diff touches `espforge/tests/golden/` says why in its PR; cosmetic re-emission is a failure, not a re-bless.
- **Every rejection has a `compile_fail` case.** The codes in §5 are deliverables.
- **Every bundled manifest has an `example`** — `parse_driver` requires one and `manifests.rs` exercises it.
- **Known baseline defect:** `cargo test --workspace` fails `every_declared_cell_is_byte_identical_to_its_golden` because goldens pin `espforge-runtime = "0.9.0"` while the workspace is `0.9.3` (see `plandriverfixes.md` §6). Judge each milestone by "no *new* churn", not by a green golden run.

---

## 7. Risk register

| # | Risk | Milestone | Mitigation |
|---|---|---|---|
| R1 | `esp-hal 1.2.2`'s RMT API differs from `12-rmt.md`'s description (ownership, `with_pin`, channel index in the type) | M0 | The whole point of M0: record signatures from source, type-check a probe before writing any IR |
| R2 | `Device::Rmt` adds an exhaustive-match arm across `model.rs` and test fixtures | M2 | Grep `Device::` first; keep `as_bus()` returning `Option` |
| R3 | Partial move of `rmt0_bus.channel{n}` breaks because something later touches the whole `Rmt` | M2/M3 | `#handle` stays a local; documented invariant; one shared builder, not two |
| R4 | `device_once` ordering puts the `Rmt::new` `let` after a channel use | M2 | Every channel builder calls `device_once` first; add an emission unit test asserting one `Rmt::new` and correct order |
| R5 | Channel pool double-booked on `esp32`/`esp32s2` | M2 | Claim by index, not `(index, direction)` (C4) |
| R6 | Diagnostic numbering collides with the ledc/pcnt plans | M1 | Fresh `E0530` block; extend the range test to `501..=532` / `901..=913` (C3) |
| R7 | WS2812 timings wrong on `esp32h2` (32 MHz source) | M3 | Compute ticks from `base_mhz`; host test the encoder's tick table for both 80 and 32 MHz |
| R8 | Golden churn from new examples/fixtures looks like accidental re-emission | M2/M3 | New fixtures only; re-bless in the milestone's own commit with the list of changed projects |

---

## 8. Immediate next step

**M0, in one PR.** It is small, touches no schema, and settles the two facts (cross-crate gating, `transmit` ownership) that would otherwise be re-worked in M2. Start with the `rmt` feature and the three target checks, then write the API table into `espforge-runtime/src/hw/rmt.rs`'s module docs — M1's element and M2's runtime are both written against it.

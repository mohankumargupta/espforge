
# `rmt.md`: Adding ESP32 RMT Peripheral & Driver Support

**Status:** proposed implementation plan  
**Baseline:** `espforge` workspace `0.9.3` (`esp-hal = "~1.2.2"`, `esp-metadata-generated = "0.5.3"`)  
**Purpose:** expose `esp_hal::rmt` as a first-class `# peripherals` controller (`<rmt>`) and device bus (`bus "rmt"`), supporting both raw pulse-sequence generation (IR TX/RX, custom pulse trains) and manifest-driven RMT devices (such as WS2812/NeoPixel strips and DShot ESCs) across blocking and Embassy runtimes.

---

## 1. Why RMT Needs Its Own Design (How It Differs from I2C, SPI, and UART)

In `espforge`, `<i2c>`, `<spi>`, and `<uart>` each map 1-to-1 to a single controller instance (`I2C0`, `SPI2`, `UART1`). The RMT hardware does not fit that shape in four ways:

1. **One `RMT` peripheral owns multiple independent channels.**  
   `esp_hal::rmt::Rmt::new(peripherals.RMT, freq)` consumes the single `RMT` block once, then exposes `rmt.channel0`, `rmt.channel1`, … (`ChannelCreator`) which are configured independently with different pins, clock dividers, and carrier settings. Two `<ws2812>` strips or an IR transmitter + IR receiver must share **one** `<rmt id="rmt0" />` without rebuilding `Rmt::new` twice.
2. **Channel count and direction are chip-specific.**  
   Unlike `I2C` or `SPI`, not every chip has RMT, and on newer RISC-V / S3 chips channels have fixed directions (TX-only vs. RX-only) while older chips allow any channel to be TX or RX:
   - **No RMT (`!soc_has_rmt`):** `esp32c2`, `esp32c61`.
   - **TX-only + RX-only split:** `esp32c3` / `esp32c6` / `esp32h2` (`channel0..1` TX, `channel2..3` RX); `esp32s3` (`channel0..3` TX, `channel4..7` RX).
   - **Bidirectional channels:** `esp32` (`channel0..7` TX or RX); `esp32s2` (`channel0..3` TX or RX).
3. **Base clock frequency depends on the chip.**  
   `Rmt::new(peripherals.RMT, freq)` takes the peripheral source frequency (`80 MHz` on `esp32`, `esp32s2`, `esp32s3`, `esp32c3`, `esp32c6`; `32 MHz` on `esp32h2`). Emitting a hardcoded `80 MHz` breaks `esp32h2` at runtime, so the chip table must supply the base clock (or validate a user-overridden clock).
4. **One `DriverMode` per `RMT` peripheral.**  
   `Rmt::new` creates `Rmt<'static, Blocking>`, and `.into_async()` converts the whole peripheral to `Rmt<'static, Async>`. Unlike `I2cBus` and `SpiBus` (where `I2C0` can be blocking while `I2C1` is async), all channels split from one `Rmt` share the same `DriverMode`.

---

## 2. Target ESPF Surface

### 2.1 Peripheral Declaration (`# peripherals`)

Following the silicon-boundary rule in `research/03-peripherals.md`, `# peripherals` owns the `RMT` controller:

```xml
# peripherals
  <rmt id="rmt0" />
```

Optional attributes on `<rmt>`:
- `frequency` (`kind="freq"`, optional): base RMT clock (defaults to chip base clock: `80MHz`, or `32MHz` on `esp32h2`).

### 2.2 Direct / Raw Channel Use (No Device Driver)

Just as an unattached `<i2c>` or `<uart>` emits a raw handle under its peripheral id (`raw_bus.rs`), users often want direct `PulseCode` TX/RX access (e.g. NEC IR remote or custom timing) without writing a third-party driver crate. Because one `<rmt>` has multiple channels, raw channels are declared explicitly in `# devices` (or as `<rmt_tx>` / `<rmt_rx>` pin/channel elements) or bound via `bus="$rmt0"`:

```xml
# peripherals
  <rmt id="rmt0" />

# devices
  <rmt_tx id="ir_tx" bus="$rmt0" pin="4" clk_divider="255" carrier="38kHz" />
  <rmt_rx id="ir_rx" bus="$rmt0" pin="5" clk_divider="255" />
```

*(Alternative if you want a completely self-contained raw peripheral without `# devices`: `<rmt id="rmt0" />` only emits raw channels when `<rmt_tx>`/`<rmt_rx>` elements exist, keeping `# peripherals` strictly about the silicon block and `# devices` about pin-bound endpoints.)*

### 2.3 Device Drivers Binding to RMT (`bus "rmt"`)

A `format_version 2` manifest (e.g. a WS2812/NeoPixel driver) declares `bus "rmt"`:

```kdl
driver "ws2812_rmt" {
    format_version 2
    summary "WS2812 / NeoPixel addressable RGB LED strip over RMT"
    category "display"

    dependency "smart-leds" { version "0.4" }

    bus "rmt" {
        direction "tx"
        clk_divider 1
    }

    codegen {
        feature "ws2812-rmt"
        runtime "blocking" {
            type "espforge_drivers::Ws2812Rmt<{leds}>"
            constructor "espforge_drivers::Ws2812Rmt::new({bus_device})"
            endpoint "espforge_runtime::BlockingRmtTxEndpoint"
        }
        runtime "async" {
            feature "ws2812-rmt-async"
            type "espforge_drivers::AsyncWs2812Rmt<{leds}>"
            constructor "espforge_drivers::AsyncWs2812Rmt::new({bus_device})"
            endpoint "espforge_runtime::AsyncRmtTxEndpoint"
        }
    }

    example "minimal" #"<ws2812_rmt id="strip" bus="$rmt0" pin="8" />"#
}
```

When `bus "rmt"` is declared in a v2 manifest, `parse_driver` synthesizes:
- `bus` (`AttrKind::Ref { to: ["rmt"] }`, required)
- `pin` (`AttrKind::PinOrRef`, required — the TX output or RX input GPIO)
- `channel` (`AttrKind::U32`, optional — auto-allocated to the next free TX/RX channel on `$rmt0` when omitted, or explicitly pinned e.g. `channel="0"` when requested)

---

## 3. Codebase Integration Plan

### Phase 1: Chip Capabilities & Diagnostics

**Files:**
- `espforge-elements/src/chips.rs`
- `espforge-diagnostics/src/codes.rs`

**Changes:**
1. **`chips.rs`:** Add RMT metadata helpers (matching `has_wifi`, `i2c_instances`, `uart_instances`):
   - `pub fn has_rmt(name: &str) -> bool` — read `chip.contains("soc_has_rmt")` from `esp-metadata-generated`.
   - `pub struct RmtCaps { pub tx_channels: &'static [u8], pub rx_channels: &'static [u8], pub base_mhz: u32 }`
   - `pub fn rmt_caps(chip: &str) -> Option<RmtCaps>`:
     - `esp32`: TX `0..=7`, RX `0..=7` (shared pool of 8), `80 MHz`
     - `esp32s2`: TX `0..=3`, RX `0..=3` (shared pool of 4), `80 MHz`
     - `esp32s3`: TX `0..=3`, RX `4..=7`, `80 MHz`
     - `esp32c3`, `esp32c6`, `esp32c5`: TX `0..=1`, RX `2..=3`, `80 MHz`
     - `esp32h2`: TX `0..=1`, RX `2..=3`, `32 MHz`
     - `esp32c2`, `esp32c61`: `None`
2. **`codes.rs`:** Add diagnostic codes (and expand `the_slice_only_defines_the_codes_it_can_reach` range in `codes.rs` tests from `501..=521` to cover `522..=526`):
   - `RMT_NOT_SUPPORTED = "E0522"` (`<rmt>` on `esp32c2` / a chip without `soc_has_rmt`)
   - `RMT_INVALID_ID = "E0523"` (`id` must be `rmt0`, or a valid identifier)
   - `RMT_INVALID_FREQUENCY = "E0524"`
   - `RMT_NO_CHANNEL_AVAILABLE = "E0912"` (all TX or RX channels on `$rmt0` exhausted — note `901..=912` in `codes.rs` tests)
   - `RMT_CHANNEL_CONFLICT = "E0913"` (requested `channel="N"` already claimed or wrong direction for chip)

---

### Phase 2: Schema (`espf.schema.kdl` & `espforge-schema/src/driver.rs`)

**Files:**
- `espforge-schema/src/espf.schema.kdl`
- `espforge-schema/src/lib.rs`
- `espforge-schema/src/driver.rs`

**Changes:**
1. **`espf.schema.kdl`:**
   - Add `<rmt>` to `# peripherals`:
     - `id` (`ident`, required)
     - `frequency` (`freq`, optional — defaults to chip base clock)
   - Update `section "peripherals"` summary/description to mention `rmt`.
2. **`espforge-schema/src/lib.rs` & `driver.rs`:**
   - Extend `parse_bus` to accept `"rmt"` alongside `"i2c" | "spi" | "uart" | "none"`.
   - Inside `bus "rmt" { ... }`, parse:
     - `direction "tx" | "rx"` (required for `bus "rmt"`)
     - `clk_divider <1..=255>` (default `1`)
     - `idle_level "low" | "high"` (optional, TX default `"low"`)
     - `carrier_hz <u32>` / `duty <u8>` (optional)
   - In `add_bus_attrs` / `add_union_bus_attrs`: synthesize `pin` (`PinOrRef`, required) and `channel` (`U32`, optional).
   - Store `RmtConfig` metadata on `DeviceRecipe` (e.g. `rmt_direction`, `rmt_clk_divider`).
   - Refuse `{address}` and `{cs_pin}` on `bus "rmt"`; allow `{bus_device}`.

---

### Phase 3: Runtime Wrappers (`espforge-runtime/src/hw/rmt.rs`)

**Files:**
- `espforge-runtime/src/hw/rmt.rs` (new)
- `espforge-runtime/src/lib.rs`

**CRITICAL INVARIANTS TO PRESERVE:**
- **No hidden `static`s (`no_static_storage_in_runtime` test in `espforge-emit`):** `espforge-runtime` must never declare `static` storage inside its modules. `Rmt` and its `Channel`s are owned directly by `Devices` fields (wrapped in `AsyncMutex` on Embassy, bare on Blocking).
- **`esp-hal` `~1.2.2` API:**
  - `Rmt::new(peripherals.RMT, freq)` returns `Result<Rmt<'static, Blocking>, Error>`.
  - Under async (`feature = "embassy"`), convert via `.into_async()`.
  - Channel configuration in `esp-hal` 1.1+/1.2 uses `configure_tx(&tx_config)?.with_pin(pin)` and `configure_rx(&rx_config)?.with_pin(pin)` (note: `configure_tx` takes a reference to `TxChannelConfig`, and `.with_pin(...)` attaches the GPIO separately).
  - Pulse codes use `esp_hal::rmt::PulseCode` (newtype over `u32`, with `PulseCode::new(level1, len1, level2, len2)` and `PulseCode::end_marker()`).
- **Chip gate:** Gate `mod rmt;` with `#[cfg(soc_has_rmt)]` (check `esp_hal::rmt` availability—do **not** compile `rmt.rs` on `esp32c2`, where `esp_hal::rmt` does not exist).

**Runtime types to expose from `espforge-runtime/src/hw/rmt.rs`:**
- Re-export `pub use esp_hal::rmt::{PulseCode, RxChannelConfig, TxChannelConfig};`
- **`RmtTxChannel<Dm>`** (or `BlockingRmtTxEndpoint` / `AsyncRmtTxEndpoint` using `AnyTxChannel<'static, Dm>` or generic `Channel<'static, Dm, Tx>` via `channel.degrade()`):
  - Using `esp_hal::rmt::Channel<'static, Dm, Tx>` (type-erased via `.degrade()` or `AnyTxChannel` if `esp-hal` exposes it, or `enum RmtTxChannel` / generic wrapper) avoids leaking channel index `0..=7` into manifest `type "..."` strings!
  - Blocking methods: `transmit(&mut self, data: &[PulseCode]) -> Result<(), RmtError>` (handles `channel.transmit(data)?.wait()` and recovers the owned channel inside an `Option<Channel>` or `replace_with` pattern so `&mut self` works cleanly without moving out of `AsyncMutexGuard`).
  - Async methods (`#[cfg(feature = "embassy")]`): `async fn transmit(&mut self, data: &[PulseCode]) -> Result<(), RmtError>` using `TxChannelAsync::transmit`.
- **`RmtRxChannel<Dm>`** (mirroring TX):
  - Blocking `receive(&mut self, buf: &mut [PulseCode]) -> Result<usize, RmtError>`.
  - Async `async fn receive(&mut self, buf: &mut [PulseCode]) -> Result<usize, RmtError>`.

*(Note on `Channel::transmit` ownership in blocking mode: `esp_hal::rmt::TxChannel::transmit(self, data)` consumes `self` by value and returns `SingleShotTxTransaction<'_, Self>`, whose `.wait()` returns `(Error, Self)` or `Ok(Self)`. Wrapping it in `Option<C>` inside `RmtTxChannel` lets `RmtTxChannel::transmit(&mut self, ...)` take `&mut self`, matching `device!(ctx, strip)` ergonomics on both blocking and embassy boards.)*

---

### Phase 4: Resource Graph, Lowering & Emission

**Files:**
- `espforge-elements/src/model.rs`
- `espforge-elements/src/element.rs`
- `espforge-elements/src/peripherals/mod.rs`
- `espforge-elements/src/peripherals/rmt.rs` (new)
- `espforge-elements/src/devices/recipe.rs`
- `espforge-elements/src/lower.rs`
- `espforge-elements/src/bus_emit.rs`

**Changes:**
1. **`model.rs`:**
   - Add `OwnerKind::Rmt` (`tag() -> "rmt"`).
   - Add `Bus::Rmt { id: Ident, peripheral: String, khz: u32 }`.
   - Add `BusTransport::Rmt(RmtTransport { bus: BusId, channel: u8, direction: RmtDirection, pin: ClaimedPin, clk_divider: u8, endpoint: Option<RustType>, async_endpoint: Option<RustType> })`.
2. **`peripherals/rmt.rs`:**
   - Implement `Element` (`#[distributed_slice(ELEMENTS)]`), tag `"rmt"`.
   - Check `chips::has_rmt(cx.chip())`; if false, emit `RMT_NOT_SUPPORTED` (`E0522`).
   - Validate `id` (`rmt0` or valid Rust ident) and `cx.claim_peripheral("RMT", owner, span)?` so only one `<rmt>` can exist.
   - Resolve `frequency`: use `attrs.frequency` if present, else `chips::rmt_caps(cx.chip()).base_mhz * 1000`.
   - Push `Bus::Rmt` into `cx`.
3. **`devices/recipe.rs` (`lower_recipe`):**
   - Handle `kind == "rmt"`:
     - Claim `pin` via `attrs::pin_or_ref` as `PinUse::Output` (for TX) or `PinUse::Input` (for RX) using chip `pin_caps` (preventing output on ESP32 input-only GPIO 34–39 or flash pins!).
     - Allocate `channel`: if `channel="..."` is specified, verify it exists in `rmt_caps.tx_channels` / `rx_channels` and claim `"RMT.channel{N}"` via `cx.claim_peripheral(...)` (so two devices cannot steal the same hardware channel, and transactional `rollback` frees it if lowering fails!). If omitted, pick the lowest unclaimed channel matching the required direction.
     - Emit `BusTransport::Rmt(...)`.
4. **`lower.rs`:**
   - `check_bus_modes` already enforces no blocking-only + async-only mix on the same `BusId`. Keep this invariant for `Bus::Rmt`.
   - Also verify `<rmt>` mode matches `Uart`-style single `DriverMode`: if `board.bus_mode(rmt_bus) == BusMode::Async`, every device on `$rmt0` must support async (otherwise `check_bus_modes` refuses it).
5. **`bus_emit.rs`:**
   - Add `rmt_device(cx, board, rmt_transport)`:
     - Use `cx.device_once(&handle, ...)` to emit `let rmt0_bus = esp_hal::rmt::Rmt::new(unsafe { registry.peripherals.RMT.clone_unchecked() }, esp_hal::time::Rate::from_khz(#khz)).expect("RMT configuration failed")[.into_async()];` once inside `Devices::new`.
     - Notice: `Rmt` channels (`rmt0_bus.channel0`, `rmt0_bus.channel1`, …) are moved out of `rmt0_bus` field-by-field as each device builds its `RmtTxChannel` / `RmtRxChannel` endpoint! Because `rmt0_bus` is only a local in `Devices::new`, moving `rmt0_bus.channel0` and `rmt0_bus.channel2` into separate `Devices` fields compiles cleanly with zero `RefCell` or `Mutex` overhead across channels.
     - Construct the pin (`OutputPin` / `InputPin` or raw `GPIO{pin}` passed to `.with_pin(...)`), apply `TxChannelConfig` / `RxChannelConfig` (`clk_divider`, `idle_output`, `carrier`), and return the endpoint expression.

---

### Phase 5: Built-in Raw `<rmt_tx>` / `<rmt_rx>` Elements & First Driver

1. Either implement `<rmt_tx>` and `<rmt_rx>` as thin bundled `driver` manifests in `espforge-drivers/manifests/` (using `bus "rmt"` + `espforge_runtime::RmtTxChannel` / `RmtRxChannel`, with `feature ""` or directly in `espforge-runtime`), **or** as built-in elements in `espf.schema.kdl` so users need no extra crate for NEC IR / raw pulse sequences.
2. Add one real device manifest (e.g. `espforge-drivers/manifests/ws2812.kdl` or `ir_tx.kdl`) with `example` fragments for both blocking and embassy.
3. Add embedded examples under `espforge-examples/src/examples/`:
   - `03.Analog` or `04.Communication` / `rmt_tx` (blocking)
   - `rmt_tx_async` (embassy)

---

## 4. Verification Checklist

Run the workspace gates in order (matching `plandriverfixes.md` §6):

1. **Runtime target check on both RMT and non-RMT chips:**
   ```sh
   # RMT-capable chip (blocking + embassy)
   cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
     --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart
   cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
     --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,embassy

   # Non-RMT chip (`!soc_has_rmt` guard must compile cleanly)
   cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
     --features esp-hal/esp32c2,esp-println/esp32c2,esp-println/uart
   ```
2. **Host unit & schema tests:**
   - `chips::has_rmt("esp32c2") == false`, `chips::has_rmt("esp32c3") == true`, `chips::rmt_caps("esp32h2").base_mhz == 32`.
   - `peripherals/rmt` rejects `esp32c2` (`E0522`) and duplicate `<rmt>` (`E0704`).
   - `lower_recipe` rejects TX on `channel="2"` (`esp32c3`, `E0913`), duplicate channel claim (`E0913`), channel exhaustion (`E0912`), and pin conflict on TX/RX GPIO (`E0701`), rolling back claims cleanly on failure.
   - `no_static_storage_in_runtime` (`espforge-emit`) stays green.
3. **Full compile & regression suite:**
   ```sh
   cargo test --workspace
   cargo test -p espforge-elements --test manifests
   cargo test -p espforge --test golden_projects -- --ignored
   cargo test -p espforge --test generated_projects_compile -- --ignored
   cargo test -p espforge --test compile_fail -- --ignored
   ```

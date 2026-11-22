
# `pcnt.md`: Adding ESP32 PCNT (Pulse Counter) Peripheral & Device Support

**Status:** proposed implementation plan  
**Baseline:** `espforge` workspace `0.9.3` (`esp-hal = "~1.2.2"`, `esp-metadata-generated = "0.5.3"`) [Cargo.toml]  
**Purpose:** expose `esp_hal::pcnt` as a `# peripherals` controller (`<pcnt>`) and device bus (`bus "pcnt"`), enabling hardware pulse counting and quadrature rotary encoders (`<pulse_counter>`, `<rotary_encoder>`) with zero CPU polling overhead across blocking and Embassy runtimes.

---

## 1. Hardware & `esp-hal` Constraints That Shape the Design

Unlike `<i2c>`, `<spi>`, and `<uart>`, `esp_hal::pcnt` has four hardware-specific properties that `espforge` must model explicitly:

1. **Not all ESP32 chips have PCNT.**  
   `esp-metadata-generated` defines `soc_has_pcnt` per chip:
   - **Supported (`soc_has_pcnt`):** `esp32` (8 units: `unit0..unit7`), `esp32s2`, `esp32s3`, `esp32c6`, `esp32h2`, `esp32c5`, `esp32p4` (4 units: `unit0..unit3`).
   - **Absent (`!soc_has_pcnt`):** `esp32c2`, `esp32c3`, `esp32c61`.
   *(Important: every existing `espforge` example defaults to `esp32c3`; PCNT examples and compile tests must target `esp32c6` or `esp32s3`, and `<pcnt>` on `esp32c3`/`esp32c2` must fail at lowering with a clear diagnostic.)*
2. **One `PCNT` peripheral owns multiple independent `Unit`s.**  
   `let pcnt = Pcnt::new(peripherals.PCNT);` consumes the `PCNT` singleton once and exposes `pcnt.unit0`, `pcnt.unit1`, …. Each `Unit` is an independent 16-bit signed hardware counter with two channels (`channel0`, `channel1`):
   - A single-channel pulse counter (flow meter, tachometer) uses 1 unit (`channel0`).
   - A quadrature rotary encoder uses 1 unit with both `channel0` and `channel1` cross-wired to the same two input pins (`A` and `B`).
3. **`Unit<'d, const UNIT_NUM: usize>` is const-generic over the unit index.**  
   `pcnt.unit0` has type `Unit<'static, 0>`, `pcnt.unit1` has type `Unit<'static, 1>`, etc. If a manifest `type "..."` named `Unit` directly, auto-allocating units would change the field's Rust type. `espforge-runtime` should therefore erase the unit index behind an enum/handle (`PcntUnit`) so the device field type stays stable regardless of which unit slot was allocated.
4. **Input signals use `Input::peripheral_input()` (cloneable `InputSignal`).**  
   In `esp-hal` 1.x, a single `Input<'static>` provides `.peripheral_input()` (an `InputSignal` that implements `Clone`), allowing a quadrature encoder to feed `pin_a` and `pin_b` into both `channel0` (edge=B, ctrl=A) and `channel1` (edge=A, ctrl=B) while claiming each physical GPIO once in `ResourceGraph`.

---

## 2. Target ESPF Surface

### 2.1 Peripheral Declaration (`# peripherals`)

`<pcnt>` claims the single `PCNT` hardware block on the silicon:

```xml
# peripherals
  <pcnt id="pcnt0" />
```

### 2.2 Built-in / Manifest Devices (`# devices`)

Multiple counter devices can share one `$pcnt0` peripheral, each claiming a free hardware unit (`unit0..unitN`):

```xml
# peripherals
  <pcnt id="pcnt0" />

# devices
  <pulse_counter id="flow" bus="$pcnt0" pin="4" edge="rising" pull="up" filter_ns="1000" />
  <rotary_encoder id="knob" bus="$pcnt0" pin_a="5" pin_b="6" pull="up" filter_ns="1000" />
```

Optional attributes:
- `unit` (`kind="u32"`, optional): pin a specific unit (`unit="0"`), or omit to auto-allocate the next free `Unit` on `$pcnt0`.
- `low_limit` / `high_limit` (`i16`, optional): hardware counter bounds (`-32768..=32767`).
- `filter_ns` (`u32`, optional): glitch filter window in nanoseconds (converted to APB cycles, clamped to hardware max `1023`).
- `pull` (`"up" | "down" | "none"`, default `"none"`): input pull resistor on the signal/control pin(s).

---

## 3. Codebase Integration Plan

### Phase 1: Chip Capabilities & Diagnostics

**Files:**
- `espforge-elements/src/chips.rs`
- `espforge-diagnostics/src/codes.rs`

**Changes:**
1. **`chips.rs`:** Add PCNT metadata helpers (mirroring `has_wifi`):
   - `pub fn has_pcnt(name: &str) -> bool` — check `chip_config(name).is_some_and(|chip| chip.contains("soc_has_pcnt"))`.
   - `pub fn pcnt_units(chip: &str) -> usize`:
     - `"esp32"` => `8`
     - `"esp32s2" | "esp32s3" | "esp32c6" | "esp32h2" | "esp32c5" | "esp32p4"` => `4`
     - everything else (`"esp32c2"`, `"esp32c3"`, `"esp32c61"`) => `0`
2. **`codes.rs`:** Add diagnostic codes (and update `the_slice_only_defines_the_codes_it_can_reach` in `codes.rs` tests to include the new codes):
   - `PCNT_NOT_SUPPORTED = "E0522"` (`<pcnt>` on `esp32c3`, `esp32c2`, or `esp32c61`)
   - `PCNT_INVALID_ID = "E0523"` (`id` must be `pcnt0` or a valid identifier)
   - `PCNT_NO_UNIT_AVAILABLE = "E0912"` (more than 4/8 devices bound to `$pcnt0`)
   - `PCNT_UNIT_CONFLICT = "E0913"` (explicit `unit="N"` already claimed or out of range for the chip)
   - `PCNT_INVALID_FILTER = "E0914"` (filter or limit out of hardware range)

---

### Phase 2: Runtime Wrappers (`espforge-runtime/src/hw/pcnt.rs`)

**Files:**
- `espforge-runtime/src/hw/pcnt.rs` (new)
- `espforge-runtime/src/lib.rs`

**CRITICAL INVARIANTS TO PRESERVE:**
- **No hidden `static`s (`no_static_storage_in_runtime` test in `espforge-emit`):** Do **not** follow `esp-hal`'s interrupt example of keeping a module-level `static UNIT0: Mutex<RefCell<Option<Unit>>>` inside `espforge-runtime`. Own the `Unit` directly inside the device struct (wrapped in `AsyncMutex` on Embassy by the emitter, bare `&mut` on Blocking).
- **Chip gate:** Gate `mod pcnt;` with `#[cfg(soc_has_rmt_or_pcnt)]` -> specifically `#[cfg(soc_has_pcnt)]` (or `esp-hal`'s chip features matching `has_pcnt`), so `espforge-runtime` still compiles cleanly for `esp32c3` and `esp32c2`.
- **Note on `esp-hal` `~1.2.2` API:** `esp_hal::pcnt` requires the `unstable` feature on `esp-hal` (already enabled by `unstable-hal` default option).

**Runtime types in `espforge-runtime/src/hw/pcnt.rs`:**

1. **Type-erased unit wrapper (`PcntUnit`):**
   ```rust
   pub enum PcntUnit {
       U0(esp_hal::pcnt::unit::Unit<'static, 0>),
       U1(esp_hal::pcnt::unit::Unit<'static, 1>),
       U2(esp_hal::pcnt::unit::Unit<'static, 2>),
       U3(esp_hal::pcnt::unit::Unit<'static, 3>),
       #[cfg(esp32)]
       U4(esp_hal::pcnt::unit::Unit<'static, 4>),
       #[cfg(esp32)]
       U5(esp_hal::pcnt::unit::Unit<'static, 5>),
       #[cfg(esp32)]
       U6(esp_hal::pcnt::unit::Unit<'static, 6>),
       #[cfg(esp32)]
       U7(esp_hal::pcnt::unit::Unit<'static, 7>),
   }
   ```
   Expose uniform methods across all variants: `count(&self) -> i16` (via `unit.counter.get()`), `clear(&self)`, `pause(&self)`, `resume(&self)`, `set_limits(...)`, `set_filter(...)`.

2. **High-level device types (both `Send + Sync`, working identically on Blocking and Embassy):**
   - **`PulseCounter`**: owns `PcntUnit` + keeps `Input<'static>` alive.
     - `pub fn new(...) -> Result<Self, DeviceErrorKind>`
     - Configures `channel0` (`set_edge_signal`, `set_input_mode` for rising/falling/both), applies optional glitch filter & limits, clears, and resumes.
     - Methods: `count(&self) -> i16`, `clear(&self)`, `pause(&self)`, `resume(&self)`.
   - **`RotaryEncoder`**: owns `PcntUnit` + keeps `Input<'static>` (`pin_a`, `pin_b`) alive.
     - Configures `channel0` and `channel1` in quadrature decoding (`CtrlMode::Reverse` / `CtrlMode::Keep` and opposing `EdgeMode::Increment` / `EdgeMode::Decrement`).
     - Applies glitch filter (e.g. default `1000ns` / user-specified `filter_ns`), clears, and resumes.
     - Methods: `count(&self) -> i16` (raw 4x quadrature steps), `steps(&self) -> i16` (`count() / 4`, matching standard detent encoders), `clear(&self)`.

*(Note on 32-bit overflow extension: if software 32-bit accumulation via PCNT high/low limit interrupts is added later, follow `I2cBusStorage` / `OtaStorage`—emit a `static __KNOB_STORAGE: PcntOverflowStorage = PcntOverflowStorage::new();` in `src/generated.rs` and pass it into `new_in(&__KNOB_STORAGE, ...)` so `espforge-runtime` keeps zero hidden `static`s.)*

---

### Phase 3: Schema (`espf.schema.kdl` & `espforge-schema/src/driver.rs`)

**Files:**
- `espforge-schema/src/espf.schema.kdl`
- `espforge-schema/src/lib.rs`
- `espforge-schema/src/driver.rs`
- `espforge-drivers/manifests/pulse_counter.kdl` & `rotary_encoder.kdl` (or built-in `# devices`)

**Changes:**
1. **`espf.schema.kdl`:**
   - Add `<pcnt>` under `# peripherals`:
     - `id` (`ident`, required)
   - Update `section "peripherals"` summary/description to include `pcnt`.
2. **`espforge-schema/src/driver.rs` & `lib.rs`:**
   - Extend `parse_bus` to accept `"pcnt"` alongside `"i2c" | "spi" | "uart" | "none"`.
   - Inside `bus "pcnt"`, allow optional `unit` handling (`unit` attribute synthesized as optional `AttrKind::U32`, `range="0..=7"`).
   - Keep `pin` declarations (`pin "pin"`, or `pin "pin_a"` + `pin "pin_b"` with `direction="input"`) as ordinary `PinDecl`s—since `PinOrRef` already supports bare pin numbers or `$<gpio>` refs!

---

### Phase 4: Resource Graph, Lowering & Emission

**Files:**
- `espforge-elements/src/model.rs`
- `espforge-elements/src/peripherals/mod.rs`
- `espforge-elements/src/peripherals/pcnt.rs` (new)
- `espforge-elements/src/devices/recipe.rs`
- `espforge-elements/src/lower.rs`
- `espforge-elements/src/bus_emit.rs`

**Changes:**
1. **`model.rs`:**
   - Add `OwnerKind::Pcnt` (`tag() -> "pcnt"`).
   - Add `Bus::Pcnt { id: Ident, peripheral: String }`.
   - Add `BusTransport::Pcnt(PcntTransport { bus: BusId, unit: u8, endpoint: Option<RustType>, async_endpoint: Option<RustType> })`.
2. **`peripherals/pcnt.rs`:**
   - Register `Pcnt` via `#[distributed_slice(ELEMENTS)]`, tag `"pcnt"`.
   - Check `chips::has_pcnt(cx.chip())`; if false, report `PCNT_NOT_SUPPORTED` (`E0522`) naming the chip (e.g. `esp32c3 has no PCNT peripheral`).
   - Claim `"PCNT"` via `cx.claim_peripheral("PCNT", owner, span)?` so two `<pcnt>` peripherals clash with `CONFLICT_PERIPHERAL` (`E0704`).
   - Add `Bus::Pcnt` to `cx`.
3. **`devices/recipe.rs` (`lower_recipe`):**
   - Handle `kind == "pcnt"`:
     - Query `chips::pcnt_units(cx.chip())`.
     - If `unit` is explicitly specified, check `unit < count`, then claim `"PCNT.unit{unit}"` via `cx.claim_peripheral(...)` (reporting `PCNT_UNIT_CONFLICT` on clash, and automatically participating in transactional `rollback`).
     - If `unit` is omitted, pick the lowest `0..count` whose `"PCNT.unit{u}"` is unclaimed and claim it via `cx.claim_peripheral(...)`; if none remain, report `PCNT_NO_UNIT_AVAILABLE`.
     - Auxiliary pins (`pin` or `pin_a` + `pin_b`) continue using existing step-6 pin claiming (`PinUse::Input`), which automatically checks `pin_caps` (allowing ESP32 input-only pins 34–39 while rejecting flash pins!).
4. **`bus_emit.rs`:**
   - Add `pcnt_device(cx, board, pcnt_transport)`:
     - Use `cx.device_once("pcnt_bus", ...)` to emit once inside `Devices::new`:
       ```rust
       let pcnt0_bus = esp_hal::pcnt::Pcnt::new(
           unsafe { registry.peripherals.PCNT.clone_unchecked() },
       );
       ```
     - Move the assigned unit out of the local `pcnt0_bus` (`pcnt0_bus.unit0`, `pcnt0_bus.unit1`, …) wrapped into `espforge_runtime::PcntUnit::U{N}(pcnt0_bus.unit{N})`.
     - Because `pcnt0_bus` is a local in `Devices::new` and each `unit{N}` is a distinct struct field, multiple devices on `$pcnt0` move their own `unit{N}` cleanly by value with zero shared `RefCell` or `Mutex` across units!

---

### Phase 5: Manifests & Examples

1. Add `espforge-drivers/manifests/pulse_counter.kdl` and `espforge-drivers/manifests/rotary_encoder.kdl` (or implement them directly against `espforge_runtime::PulseCounter` / `RotaryEncoder` so no external crate is needed).
2. Add examples under `espforge-examples/src/examples/02.Digital/` (or `03.Counters/`):
   - `rotary_encoder` (`chip="esp32c6"`, `runtime="blocking"`)
   - `rotary_encoder_async` (`chip="esp32c6"`, `runtime="embassy"`)

---

## 4. Verification Checklist

Run the workspace gates (remembering that PCNT target compilation must use a `soc_has_pcnt` chip like `esp32c6` or `esp32s3`, and verify `!soc_has_pcnt` builds on `esp32c3`/`esp32c2`):

1. **Runtime target check on PCNT and non-PCNT chips:**
   ```sh
   # PCNT-capable chip (blocking + embassy)
   cargo check -p espforge-runtime --target riscv32imac-unknown-none-elf \
     --features esp-hal/esp32c6,esp-println/esp32c6,esp-println/uart
   cargo check -p espforge-runtime --target riscv32imac-unknown-none-elf \
     --features esp-hal/esp32c6,esp-println/esp32c6,esp-println/uart,embassy

   # Non-PCNT chip (`!soc_has_pcnt` cfg guard must keep compiling cleanly)
   cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
     --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart
   ```
2. **Host unit & lowering tests:**
   - `chips::has_pcnt("esp32c3") == false`, `chips::has_pcnt("esp32c2") == false`, `chips::has_pcnt("esp32c6") == true`, `chips::pcnt_units("esp32") == 8`, `chips::pcnt_units("esp32s3") == 4`.
   - `peripherals/pcnt` rejects `esp32c3` (`E0522`) and duplicate `<pcnt>` (`E0704`).
   - `lower_recipe` auto-allocates `unit0`, `unit1`, … across multiple devices on `$pcnt0`, rejects `unit="4"` on `esp32c6` (`E0913`), rejects duplicate explicit `unit="0"` (`E0913`), rejects >4 units on `esp32c6` (`E0912`), and allows ESP32 input-only pins (`GPIO34`/`GPIO35`) for `pin_a`/`pin_b`.
   - `no_static_storage_in_runtime` (`espforge-emit`) stays green.
3. **Full workspace & compile gates:**
   ```sh
   cargo test --workspace
   cargo test -p espforge-elements --test manifests
   cargo test -p espforge --test golden_projects -- --ignored
   cargo test -p espforge --test generated_projects_compile -- --ignored
   cargo test -p espforge --test compile_fail -- --ignored
   ```

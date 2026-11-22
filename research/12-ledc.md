
# `ledc.md`: Adding ESP32 LEDC (PWM) Peripheral & Device Support

**Status:** proposed implementation plan  
**Baseline:** `espforge` workspace `0.9.3` (`esp-hal = "~1.2.2"`, `esp-metadata-generated = "0.5.3"`)  
**Purpose:** expose `esp_hal::ledc` as a `# peripherals` timer/controller (`<ledc>`) and device bus (`bus "ledc"` / `"pwm"`), enabling PWM outputs (`<pwm_led>`, `<servo>`, `<buzzer>`, and external `embedded_hal::pwm::SetDutyCycle` drivers) across blocking and Embassy runtimes.

---

## 1. Hardware & `esp-hal` Constraints That Shape the Design

`esp_hal::ledc` differs from `I2C`, `SPI`, and `UART` in four ways that directly affect `espforge`'s schema, lifetime model, and storage rules:

1. **One `LEDC` peripheral owns multiple timers (`Timer0..Timer3`) and channels (`Channel0..Channel5/7`).**  
   `let mut ledc = Ledc::new(peripherals.LEDC);` consumes the single `LEDC` singleton once and sets `ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);`. Multiple timers (e.g. a 50 Hz timer for servos and a 24 kHz timer for dimmable LEDs) and multiple channels share that single `LEDC` peripheral.
2. **A `Channel` borrows its `Timer` (`Channel<'a, S>`).**  
   In `esp-hal`, `channel.configure(channel::config::Config { timer: &lstimer, ... })` binds the lifetime `'a` of `&'a Timer<'a, S>` into `Channel<'a, S>`. If a timer were a local variable inside `Devices::new(&mut registry)`, `Channel<'a, LowSpeed>` could not be moved into `Devices` (`'a` would not live for `'static`).  
   **Design consequence:** each configured timer **must** live in a `'static` storage cell declared in `src/generated.rs` (`LedcTimerStorage`, matching `I2cBusStorage` / `OtaStorage` and preserving `no_static_storage_in_runtime`), so `Channel<'static, LowSpeed>` is `'static`.
3. **Channel & speed-mode counts vary by chip.**  
   - **Timers:** 4 low-speed timers (`Timer0..Timer3`) on all ESP32 chips (plus 4 high-speed timers on classic `esp32` only).
   - **Low-speed channels:**
     - `esp32`, `esp32s2`, `esp32s3`, `esp32p4`: 8 channels (`Channel0..Channel7`)
     - `esp32c2`, `esp32c3`, `esp32c5`, `esp32c6`, `esp32c61`, `esp32h2`: 6 channels (`Channel0..Channel5`)
   - **Max duty resolution:** 20-bit on `esp32`, 14-bit on `esp32s2`/`s3`/`c2`/`c3`/`c6`/`h2`/`c5`/`p4`.
   - **High-speed mode:** `esp32` only (`HighSpeed`); using `LowSpeed` uniformly covers every chip in the workspace.
4. **Standard `embedded-hal` PWM trait is already implemented.**  
   `esp_hal::ledc::channel::Channel<'a, S>` implements `embedded_hal::pwm::SetDutyCycle` (`max_duty_cycle`, `set_duty_cycle`, `set_duty_cycle_fully_on`, `set_duty_cycle_fully_off`), plus inherent `set_duty(duty_pct)` and `start_duty_fade(...)`. Wrapping `Channel<'static, LowSpeed>` in `espforge_runtime::PwmChannel` lets external crates take `SetDutyCycle` directly while keeping channel numbers (`0..=7`) out of manifest `type "..."` strings.

---

## 2. Target ESPF Surface

### 2.1 Peripheral Declarations (`# peripherals`)

Following `research/03-peripherals.md` §7, `# peripherals` declares **timers** on the `LEDC` controller—because multiple PWM channels share one timer frequency and duty resolution:

```xml
# peripherals
  <ledc id="pwm_leds" frequency="24kHz" resolution="8bit" />
  <ledc id="pwm_servos" frequency="50Hz" resolution="14bit" />
```

Attributes on `<ledc>`:
- `id` (`ident`, required): referenced by devices via `bus="$pwm_leds"` (or `timer="$pwm_servos"`).
- `frequency` (`freq`, default `"1kHz"`): PWM waveform frequency.
- `resolution` (`choice`, default `"10bit"`): `"5bit"`..`"14bit"` (checked against chip max bits and frequency compatibility).
- `timer` (`u32`, optional, `range="0..=3"`): explicit hardware `Timer0..Timer3`, or auto-allocated in declaration order (up to 4 timers per chip).

### 2.2 Built-in / Manifest Devices (`# devices`)

Multiple PWM devices can share one `<ledc>` timer (same frequency, independent duty cycles) or bind to separate `<ledc>` timers (different frequencies):

```xml
# peripherals
  <ledc id="pwm_leds" frequency="24kHz" resolution="8bit" />
  <ledc id="pwm_servos" frequency="50Hz" resolution="14bit" />

# devices
  <pwm_led id="status_led" bus="$pwm_leds" pin="18" duty="0" />
  <servo   id="claw"       bus="$pwm_servos" pin="4" />
  <servo   id="arm"        bus="$pwm_servos" pin="5" />
```

A `format_version 2` manifest can also bind to `bus "ledc"` for any external `embedded-hal` `SetDutyCycle` driver (e.g. a buzzer, DC motor H-bridge, or backlight controller).

---

## 3. Codebase Integration Plan

### Phase 1: Chip Capabilities & Diagnostics

**Files:**
- `espforge-elements/src/chips.rs`
- `espforge-diagnostics/src/codes.rs`

**Changes:**
1. **`chips.rs`:** Add LEDC capability helpers:
   - `pub fn has_ledc(name: &str) -> bool` — check `chip_config(name).is_some_and(|c| c.contains("soc_has_ledc"))` (true for all current Espressif targets).
   - `pub struct LedcCaps { pub timers: u8, pub channels: u8, pub max_duty_bits: u8 }`
   - `pub fn ledc_caps(chip: &str) -> Option<LedcCaps>`:
     - `"esp32"` => `{ timers: 4, channels: 8, max_duty_bits: 20 }` (using `LowSpeed` uniformly; 8 HS channels exist on `esp32` if added later)
     - `"esp32s2" | "esp32s3" | "esp32p4"` => `{ timers: 4, channels: 8, max_duty_bits: 14 }`
     - `"esp32c2" | "esp32c3" | "esp32c5" | "esp32c6" | "esp32c61" | "esp32h2"` => `{ timers: 4, channels: 6, max_duty_bits: 14 }`
2. **`codes.rs`:** Add diagnostic codes (and expand `the_slice_only_defines_the_codes_it_can_reach` range in `codes.rs` tests):
   - `LEDC_NOT_SUPPORTED = "E0522"` (chip has no LEDC peripheral)
   - `LEDC_INVALID_FREQUENCY = "E0523"` (unparseable or out-of-range frequency)
   - `LEDC_NO_TIMER_AVAILABLE = "E0524"` (more than 4 `<ledc>` timers declared)
   - `LEDC_TIMER_CONFLICT = "E0525"` (duplicate explicit `timer="N"`)
   - `LEDC_RESOLUTION_UNSUPPORTED = "E0526"` (e.g. `16bit` requested on `esp32c3`, max 14-bit)
   - `LEDC_NO_CHANNEL_AVAILABLE = "E0912"` (all 6 or 8 channels on `LEDC` exhausted)
   - `LEDC_CHANNEL_CONFLICT = "E0913"` (duplicate explicit `channel="N"` or out of range)

---

### Phase 2: Runtime Wrappers (`espforge-runtime/src/hw/ledc.rs`)

**Files:**
- `espforge-runtime/src/hw/ledc.rs` (new)
- `espforge-runtime/src/lib.rs`

**CRITICAL INVARIANTS TO PRESERVE:**
- **No hidden `static`s (`no_static_storage_in_runtime` test in `espforge-emit`):** Never put a `static` inside `espforge-runtime`. Instead, export `LedcTimerStorage` so `generated.rs` declares one `static __<ID>_STORAGE: espforge_runtime::LedcTimerStorage = espforge_runtime::LedcTimerStorage::new();` per `<ledc>` timer.
- **Lifetime soundness (`Channel<'static, LowSpeed>`):**
  ```rust
  pub struct LedcTimerStorage {
      cell: static_cell::StaticCell<esp_hal::ledc::timer::Timer<'static, esp_hal::ledc::LowSpeed>>,
  }
  ```
  `LedcTimer::build_in(storage: &'static LedcTimerStorage, timer: Timer<'static, LowSpeed>, config: LedcTimerConfig) -> Result<LedcTimer, ...>` initializes `storage.cell` and returns a `Copy` handle wrapping `&'static Timer<'static, LowSpeed>`.
- **Verify `esp-hal` `~1.2.2` field names & `Send` bounds:**
  - `channel::config::Config` takes `timer: &'static Timer<'static, LowSpeed>`, `duty_pct: u8`, and `drive_mode: esp_hal::gpio::DriveMode` (in `esp-hal` `1.2.x`; verify against `pin_config` if targeting `1.0`).
  - Verify `Channel<'static, LowSpeed>: Send` so Embassy projects can store `PwmChannel` inside `AsyncMutex<PwmChannel>` in `Devices`. (If `Timer` is `!Sync`, wrap `Timer` or `Channel` safely behind a `critical_section` or `unsafe impl Send` only after verifying `esp-hal`'s hardware register ownership guarantees that each `Channel` only mutates its own channel registers.)

**Runtime types in `espforge-runtime/src/hw/ledc.rs`:**
- **`LedcTimerStorage`** and **`LedcTimer`** (`Copy`, holds `&'static Timer<'static, LowSpeed>`).
- **`PwmChannel`** (wraps `esp_hal::ledc::channel::Channel<'static, esp_hal::ledc::LowSpeed>`):
  - `pub fn build(...) -> Result<Self, DeviceErrorKind>`
  - `pub fn set_duty_pct(&mut self, duty_pct: u8) -> Result<(), PwmError>`
  - `pub fn start_fade(&mut self, start_pct: u8, end_pct: u8, duration_ms: u16) -> Result<(), PwmError>`
  - `pub fn is_fade_running(&self) -> bool`
  - `impl embedded_hal::pwm::ErrorType for PwmChannel`
  - `impl embedded_hal::pwm::SetDutyCycle for PwmChannel` (forwards to `Channel::set_duty_cycle` / `max_duty_cycle`)
- **`PwmLed`** (optional convenience wrapper or `PwmChannel` alias, plus `set_brightness(pct: u8)` / `fade(from, to, ms)`).
- **`Servo`** (optional `PwmChannel` wrapper exposing `set_angle(deg: u8)` mapped to 500–2500 µs pulse width via `set_duty_cycle`).

---

### Phase 3: Schema (`espf.schema.kdl` & `espforge-schema/src/driver.rs`)

**Files:**
- `espforge-schema/src/espf.schema.kdl`
- `espforge-schema/src/lib.rs`
- `espforge-schema/src/driver.rs`

**Changes:**
1. **`espf.schema.kdl`:**
   - Add `<ledc>` to `# peripherals`:
     - `id` (`ident`, required)
     - `frequency` (`freq`, default `"1kHz"`)
     - `resolution` (`choice`, default `"10bit"`: `"5bit"`..`"14bit"`)
     - `timer` (`u32`, optional, `range="0..=3"`)
   - Update `section "peripherals"` summary/description to mention `ledc`.
2. **`espforge-schema/src/driver.rs` & `lib.rs`:**
   - Extend `parse_bus` to accept `"ledc"` alongside `"i2c" | "spi" | "uart" | "none"`.
   - Inside `bus "ledc" { ... }`, parse optional `duty_pct <0..=100>` and `drive_mode "push_pull" | "open_drain"`.
   - In `add_bus_attrs` / `add_union_bus_attrs`: synthesize:
     - `pin` (`AttrKind::PinOrRef`, required — the PWM output GPIO)
     - `channel` (`AttrKind::U32`, optional — auto-allocated `0..caps.channels` when omitted)
     - `duty` (`AttrKind::U32`, optional, `range="0..=100"`, default `"0"`)
   - Refuse `{address}` and `{cs_pin}` on `bus "ledc"`; allow `{bus_device}` (which emits the configured `PwmChannel`).

---

### Phase 4: Resource Graph, Lowering & Emission

**Files:**
- `espforge-elements/src/model.rs`
- `espforge-elements/src/peripherals/mod.rs`
- `espforge-elements/src/peripherals/ledc.rs` (new)
- `espforge-elements/src/devices/recipe.rs`
- `espforge-elements/src/lower.rs`
- `espforge-elements/src/bus_emit.rs`

**Changes:**
1. **`model.rs`:**
   - Add `OwnerKind::Ledc` (`tag() -> "ledc"`).
   - Add `Bus::Ledc { id: Ident, peripheral: String, timer: u8, hz: u32, duty_bits: u8 }`.
   - Add `BusTransport::Ledc(LedcTransport { bus: BusId, channel: u8, pin: ClaimedPin, initial_duty_pct: u8, endpoint: Option<RustType>, async_endpoint: Option<RustType> })`.
2. **`peripherals/ledc.rs`:**
   - Register `Ledc` via `#[distributed_slice(ELEMENTS)]`, tag `"ledc"`.
   - Check `chips::has_ledc(cx.chip())` and `chips::ledc_caps(cx.chip())`.
   - Validate `resolution` `<= caps.max_duty_bits` (`E0526`).
   - Parse `frequency` in Hz (`parse_hz`, allowing `50Hz`, `1kHz`, `24kHz`, `1MHz`; note `parse_khz` in `peripherals/mod.rs` rejects `50Hz` because it requires whole kHz—add a `parse_hz` helper so servo `50Hz` works!).
   - Allocate `timer` `0..caps.timers`: claim `"LEDC.timer{N}"` via `cx.claim_peripheral(...)` (do **not** claim `"LEDC"` exclusively on the first `<ledc>` tag, because a board may declare up to 4 `<ledc>` timers on the same `LEDC` block!).
3. **`devices/recipe.rs` (`lower_recipe`):**
   - Handle `kind == "ledc"`:
     - Claim `pin` as `PinUse::Output` via `cx.claim_pin(...)` (which automatically prevents PWM on ESP32 input-only GPIO 34–39 and SPI flash pins!).
     - Allocate `channel` `0..caps.channels`: claim `"LEDC.channel{C}"` via `cx.claim_peripheral(...)` (auto-allocating the lowest free channel when `channel=` is omitted, or reporting `LEDC_CHANNEL_CONFLICT` if taken/out of range).
4. **`bus_emit.rs`:**
   - Add `ledc_device(cx, board, ledc_transport)`:
     - Use `cx.device_once("ledc_controller", ...)` to instantiate `Ledc` **once** inside `Devices::new`:
       ```rust
       let mut ledc_ctrl = esp_hal::ledc::Ledc::new(
           unsafe { registry.peripherals.LEDC.clone_unchecked() },
       );
       ledc_ctrl.set_global_slow_clock(esp_hal::ledc::LSGlobalClkSource::APBClk);
       ```
     - Use `cx.device_once(&format!("{}_timer", id), ...)` to configure each referenced `<ledc>` timer **once** into its module-scope `static __<ID>_STORAGE: espforge_runtime::LedcTimerStorage = espforge_runtime::LedcTimerStorage::new();`:
       ```rust
       let pwm_leds_timer = espforge_runtime::LedcTimer::build_in(
           &__PWM_LEDS_STORAGE,
           ledc_ctrl.timer::<esp_hal::ledc::LowSpeed>(esp_hal::ledc::timer::Number::Timer0),
           espforge_runtime::LedcTimerConfig { ... },
       )
       .expect("LEDC timer configuration failed");
       ```
     - Build `espforge_runtime::PwmChannel::build(ledc_ctrl.channel(channel::Number::Channel0, unsafe { registry.peripherals.GPIO18.clone_unchecked() }), pwm_leds_timer, 0)` for each device.
   - *(Note on `raw_bus.rs`:)* Unlike `<i2c>`/`<uart>`, an unused `<ledc>` timer has no pins of its own, so `raw_bus.rs` does not emit anything for an unreferenced `<ledc>` timer; PWM outputs always live in `# devices`.

---

### Phase 5: Manifests & Examples

1. Add `espforge-drivers/manifests/pwm_led.kdl` (and optionally `servo.kdl`), exposing `PwmChannel` / `Servo` with `bus "ledc"` and `init="boot"` / `"deferred"`.
2. Add an example under `espforge-examples/src/examples/03.Analog/pwm_fade/` (blocking) and `pwm_fade_async/` (embassy) showing `device!(ctx, led).set_duty_pct(...)` or `start_fade(...)`.

---

## 4. Verification Checklist

1. **Runtime target check (blocking + embassy):**
   ```sh
   cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
     --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart
   cargo check -p espforge-runtime --target riscv32imc-unknown-none-elf \
     --features esp-hal/esp32c3,esp-println/esp32c3,esp-println/uart,embassy
   ```
   Confirm `Channel<'static, LowSpeed>` satisfies `Send` for `AsyncMutex<PwmChannel>` in Embassy builds.
2. **Host unit & lowering tests:**
   - Two `<ledc>` timers (`50Hz` and `24kHz`) share one `LEDC` controller cleanly, emitting one `Ledc::new(...)` and two `LedcTimerStorage` statics.
   - `50Hz` parses cleanly (`parse_hz`, not rejected by `parse_khz`!).
   - 5th `<ledc>` timer is rejected (`E0524`); 7th channel on `esp32c3` is rejected (`E0912`); `16bit` resolution on `esp32c3` is rejected (`E0526`); PWM on ESP32 input-only `GPIO34` is rejected (`E0518`); pin conflict between `<pwm_led>` and `<led>` is caught (`E0701`).
   - `no_static_storage_in_runtime` (`espforge-emit`) stays green.
3. **Full workspace & compile gates:**
   ```sh
   cargo test --workspace
   cargo test -p espforge-elements --test manifests
   cargo test -p espforge --test golden_projects -- --ignored
   cargo test -p espforge --test generated_projects_compile -- --ignored
   cargo test -p espforge --test compile_fail -- --ignored
   ```

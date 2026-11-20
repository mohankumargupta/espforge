# `fixgaps.md`: Closing Bus, Pin, Lifecycle, and Codegen Gaps in the Driver Architecture

**Status:** proposed architecture & engineering plan  
**Baseline:** `espforge` workspace `0.9.2`, branch `dev`  
**Supersedes / extends:** builds on `driverfixes.md` (landed in `0.9.1`) and consolidates gaps from `driverglue.md`, `research/06-multitransport.md`, and `research/07-driver-external-limitations.md`.

**Purpose:** Redesign the driver manifest, lowering, runtime, and emitter layers so that:
1. **GPIO-timed / busless devices** (`hx711`, `tm1637`, `dht22`, `hc-sr04`, rotary encoders) work purely through `format_version 2` manifests without hand-written Rust elements.
2. **1-Wire multidrop buses** (`ds18b20`) and **direct analog inputs** (`<adc>`) become first-class peripherals and transports.
3. **Every remaining hole discovered in the `0.9.2` pipeline**—multi-transport manifests, CS-less SPI, `build.rs` attribute-generator panics, `then`-step fallibility, SPI CS/clock-glitch ordering, missing `embedded-hal` trait impls on runtime types, and brittle `esp-hal` type strings in manifests—is fixed systematically.

---

## Part 1: Audit of Current Gaps in `0.9.2`

| # | Gap | Where it lives in `0.9.2` | Concrete failure / blocked parts |
|---|---|---|---|
| **G1** | **Mandatory hardware bus** | `espforge-schema/src/driver.rs` (`parse_driver`, `parse_bus`, `parse_constructor`); `espforge-elements/src/model.rs` (`BusTransport`) | Every `driver` requires `bus "i2c" \| "spi" \| "uart"` and `{bus_device}` as arg 0. Blocks **HX711**, **TM1637**, **DHT11/22**, **HC-SR04**, steppers, shift registers. |
| **G2** | **Pin-only recipes are `led`/`button`-locked** | `espforge-elements/src/devices/pin.rs` (`PinRecipeElement`); `espforge-schema/src/lib.rs` (`parse_recipe`) | `recipe` supports only 1 pin (`input` or `output`) + `bool` flags (`active_low`/`pull_up`). Cannot express 2+ pins, `bidirectional`, or external crates. |
| **G3** | **Auxiliary pins lack `PinOrRef` & `embedded-hal` traits** | `espforge-schema/src/driver.rs` (`parse_pin`); `espforge-runtime/src/lib.rs` (`Bidirectional`, `Led`, `Button`) | `pin "..."` forces `$gpio` indirection in `# peripherals`; `Bidirectional` lacks `embedded_hal::digital::{InputPin, OutputPin}` impls; raw `<gpio>` is invisible in `app.rs`. |
| **G4** | **No 1-Wire peripheral or ROM addressing** | `espf.schema.kdl`, `espforge-elements/src/model.rs`, `espforge-runtime` | **DS18B20** and other 1-Wire multidrop sensors need shared line timing, critical-section reset/search, and 64-bit ROM selection (`u64` / `[u8; 8]`), not 7-bit I²C addresses. |
| **G5** | **No ADC / PWM / RMT / I2S / PCNT peripherals** | `espf.schema.kdl`, `espforge-elements/src/pin_caps.rs` | Analog sensors (thermistors, MQ sensors), servos, WS2812B LEDs, audio, and pulse counters have no silicon peripheral IR or chip capability maps (e.g. ADC2 vs Wi-Fi). |
| **G6** | **Asymmetric `Delay` traits on `embassy`** | `espforge-runtime/src/lib.rs` (`Delay`) | `Delay` implements `embedded_hal::delay::DelayNs` (blocking busy-wait) even on `embassy`, and never implements `embedded_hal_async::delay::DelayNs`. Async crates needing an async delay cannot use `Delay`. |
| **G7** | **1-to-1 single-transport driver lock-in** | `espforge-schema/src/driver.rs`; `espforge-elements/src/devices/recipe.rs` | Dual-bus chips (**BME280**, **BME680**, **ADXL345**, **BMP280**) can only expose one bus in a manifest. |
| **G8** | **SPI `cs` always required + clock glitch bug** | `espforge-schema/src/driver.rs` (`add_bus_attrs`); `espforge-runtime/src/hw/spi.rs` (`ConfiguredSpiDevice::transaction`) | CS-less SPI devices (`ws2812-spi`, `apa102`) are refused. Also, `Configured[Async]SpiDevice::transaction` asserts CS **before** calling `bus.configure()`, risking clock-polarity glitches while the slave is selected. |
| **G9** | **Exclusive UART ownership** | `espforge-elements/src/devices/recipe.rs` (`lower_recipe` `"uart"` arm) | Refuses >1 device on a UART port, blocking RS-485 / Modbus RTU / addressable daisy-chained serial devices. |
| **G10** | **`then` steps hardcode fallibility; ctor paths forbid generics & literals** | `espforge-elements/src/model.rs` (`step_statements`); `espforge-schema/src/driver.rs` (`path_text`, `parse_constructor`) | `then` steps unconditionally emit `.map_err(...)`, breaking infallible setup methods (`set_rotation()`, `clear()`). Constructor paths forbid turbofish (`Driver::<Model>::new`) and struct literals (`WS2812SPI { ... }`). |
| **G11** | **`build.rs` panics on optional `Hex`/`PinOrRef`/`U32`/`Bool` defaults** | `espforge-elements/build.rs` (`field_type`); `espforge-elements/src/attrs.rs` | Bundled manifests cannot declare `address default="0x3C"` (`(Hex, false, Some(_))` panics `build.rs`!), optional `PinOrRef`, or defaultless optional `U32`/`Bool`. |
| **G12** | **No chip/runtime/alloc constraints or tag aliases** | `espforge-schema/src/driver.rs`, `espf.schema.kdl` | Crates needing `alloc` fail silently under `<options defaults="false" />`. Variant part numbers (`sht30` vs `sht31`) have no `alias`. Nested `compose` is blocked. |
| **G13** | **Brittle `esp-hal` type strings in external manifests** | `espforge-drivers/manifests/*.kdl`, `espforge-runtime/src/lib.rs` | Every I²C/UART manifest spells out `espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>`. An `esp-hal` generic change breaks all external manifests. |

---

## Part 2: Architectural Redesign (Phased)

Every change below is **additive to `format_version 2`**: existing manifests continue to load unchanged, golden projects stay byte-identical unless an example adopts a new feature, and every template remains checked by `syn` at schema load time.

---

### Milestone 1: Busless / GPIO-Pin Drivers (`hx711`, `tm1637`, `dht22`) & Runtime Traits

*Solves G1, G2, G3, G6.*

#### 1.1 Allow `bus "none"` (Pin-Only Drivers) in `format_version 2`
In `espforge-schema/src/driver.rs`:
- Allow `bus "none"` (or omitting `bus` altogether when at least one `pin` is declared; keep requiring explicit `bus "none"` if clarity at read time is preferred).
- Synthesize `id` only (no `bus` attribute) on `Element`.
- In `parse_constructor`, when `bus` is `"none"`, drop the requirement that `{bus_device}` / `{spi_device}` be the first argument. Constructor slots may consist solely of declared `pin`s, mapped `attr`s, and fixed expressions.
- Add `BusTransport::None` (or refactor `Device::Bus` / `BusDevice` into `Device::Bus` vs `Device::Pins`, or `BusTransport::Pins(PinSet)`). Keeping `BusTransport::Pins` inside `BusDevice` (or making `BusDevice::transport` an `enum Transport { Bus(BusTransport), Pins }` so `BusMode` logic doesn't assume a `BusId`) avoids polluting `Board::bus_mode(BusId)` for busless devices:
  ```rust
  pub enum DeviceTransport {
      I2c(I2cTransport),
      Spi(SpiTransport),
      Uart(UartTransport),
      OneWire(OneWireTransport),
      Adc(AdcTransport),
      /// Busless GPIO-timed or bit-banged driver (`hx711`, `tm1637`, `dht22`).
      Pins,
  }
  ```
- Under `Pins` + `init="deferred"`, the deferred factory simply stores the claimed raw peripherals (`pin_clk: esp_hal::peripherals::GPIO4<'static>`, etc.) with no `endpoint` field required (`parse_runtime` must not require `endpoint` when `bus "none"`!).

#### 1.2 Allow `PinOrRef` and `open_drain` on `pin` Declarations
Currently, `pin "dc"` forces the user to declare `<gpio id="gpio4" pin="4" />` in `# peripherals` and reference `dc="$gpio4"`, even though SPI `cs="9"` already accepts a bare GPIO number *or* a `$gpio` ref.

1. **Extend `pin` in `espforge-schema/src/driver.rs`:**
   - Default `pin` attributes to `AttrKind::PinOrRef` (with `via="ref"` opt-in if a driver ever needs strict direction checking on a `<gpio>`), or support `pin_or_ref` natively in `devices::recipe::lower_recipe` for all `pin` slots.
   - Add `direction="open_drain"` (or `open_drain=#true` on `direction="bidirectional"`) alongside `"input"`, `"output"`, and `"bidirectional"`:
     - On `esp-hal`, open-drain outputs (`OutputOpenDrain` / `OutputConfig::default().with_drive_mode(DriveMode::OpenDrain)` + `Input` / `Flex`) are required by TM1637, DHT11/22, and 1-Wire when using external pull-ups.
   - Allow `pin` placeholders inside wrapped constructor arguments (`WRAPPED_HOLES`) or directly as `{pin_name}` / `{pin_name_pin}`.
2. **Add `opt_pin_or_ref` to `espforge-elements/src/attrs.rs`:**
   - Required so `optional=#true` works on `PinOrRef`-backed pins (e.g. `backlight="6"` or omitted).
3. **Allow `raw` `<gpio>` exposure (optional `expose=#true` or default):**
   - If a `<gpio>` is declared in `# peripherals` and *not* claimed by any device, consider emitting it in `raw_bus.rs` (opt-in via `<gpio id="dbg" pin="2" direction="output" raw="true" />` so existing projects don't change generated `Devices` fields).

#### 1.3 Implement Standard `embedded-hal` Traits in `espforge-runtime`
In `espforge-runtime/src/lib.rs`:
1. **`Bidirectional` (`Flex<'static>` wrapper):**
   - Implement `embedded_hal::digital::ErrorType` (`Error = core::convert::Infallible`).
   - Implement `embedded_hal::digital::InputPin` (`is_high`, `is_low`) and `embedded_hal::digital::OutputPin` (`set_high`, `set_low`), automatically switching `output_enable` (`true` on `set_low` / `drive` in open-drain mode, `false` on `release` / high-Z) or providing `OpenDrainPin` as a dedicated wrapper around `Flex<'static>` so crates like `tm1637-gpio-driver` and `dht-sensor` can take a single `DIO` pin directly without custom Rust glue.
   - Expose `OutputPinWrapper` / `InputPinWrapper` or re-export `esp_hal::gpio::{Input, Output, Flex}` consistently through `espforge_runtime::gpio::{Input, Output, Flex}` so external manifests don't leak `esp_hal::gpio::*` into their `type` strings.
2. **`Delay`:**
   - Under `feature = "embassy"`, implement `embedded_hal_async::delay::DelayNs` on `espforge_runtime::Delay` (using `embassy_time::Timer::after_nanos` / `after_millis`) while keeping `embedded_hal::delay::DelayNs` for blocking crates!
   - Expose an explicit blocking busy-wait helper (`Delay::delay_us_blocking`) alongside `Delay` so bit-banged drivers (HX711, DHT22, TM1637) that need microsecond slot timing inside an embassy task can use blocking microsecond delays safely without losing async `delay_ms`.
3. **Stable Endpoint & Pin Type Aliases in `espforge-runtime` (Solves G13):**
   - Export target-gated aliases from `espforge-runtime`:
     ```rust
     pub type BlockingI2cEndpoint = embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>;
     #[cfg(feature = "embassy")]
     pub type AsyncI2cEndpoint = AsyncI2cBus;
     pub type BlockingSpiEndpoint = ConfiguredSpiDevice;
     #[cfg(feature = "embassy")]
     pub type AsyncSpiEndpoint = ConfiguredAsyncSpiDevice;
     pub type BlockingUartEndpoint = UartDevice<esp_hal::Blocking>;
     #[cfg(feature = "embassy")]
     pub type AsyncUartEndpoint = UartDevice<esp_hal::Async>;
     pub type OutputPin = esp_hal::gpio::Output<'static>;
     pub type InputPin = esp_hal::gpio::Input<'static>;
     ```
   - Migrate `sht31.kdl`, `sht4x.kdl`, `ads111x.kdl`, `bme680.kdl`, `mhz19b.kdl` to use these aliases so an `esp-hal` 2.0 upgrade only changes `espforge-runtime`, not every manifest in the ecosystem.

---

### Milestone 2: First-Class Shared 1-Wire Bus (`ds18b20`) & Direct Analog Inputs (`<adc>`)

*Solves G4, G5.*

#### 2.1 Shared 1-Wire Peripheral (`<onewire>`)
DS18B20 probes frequently share one GPIO data line (multidrop). Modeling 1-Wire as a bus rather than a raw pin prevents two probes on the same wire from triggering a pin conflict.

1. **Schema (`espf.schema.kdl` & `driver.rs`):**
   - Add `<onewire id="ow0" pin="4" pull="internal|external" />` to `# peripherals` (`pull` defaults to `"external"` since 1-Wire normally uses a 4.7 kΩ pull-up, with `"internal"` enabling the weak pull-up for short bench wires).
   - Add `bus "onewire"` to `driver.rs`:
     - Synthesizes `rom` attribute (`AttrKind::Str` or new `AttrKind::Hex64` / `u64`, optional if single-drop / index-based):
       ```kdl
       bus "onewire" {
           address optional=#true   // 64-bit ROM code e.g. "0x28FF641E..." or omitted for search/first
       }
       ```
     - Add `{rom}` placeholder (emitted as `Option<u64>` or `u64` / `[u8; 8]` depending on manifest mapping or `OneWireAddress` type).
   - Add `check_onewire_roms` in `espforge-elements/src/lower.rs` (mirroring `check_i2c_addresses`) to refuse two devices on the same `<onewire>` bus claiming the same explicit ROM address.
2. **Runtime (`espforge-runtime/src/hw/onewire.rs`):**
   - Implement `OneWireBus` + `OneWireBusStorage` (following the `no_static_storage_in_runtime` contract: storage declared in `generated.rs` as `static __OW0_STORAGE: OneWireBusStorage = OneWireBusStorage::new();`).
   - Wrap a `Flex<'static>` (open-drain + input) in a `critical_section::Mutex<RefCell<...>>`.
   - Timing-critical slot operations (`reset`, `write_bit`, `read_bit`, `write_byte`, `read_byte`, `search_rom`, CRC-8) run inside `critical_section::with` using `esp_hal::delay::Delay` so interrupts or Embassy task switches cannot corrupt microsecond slots.
   - Implement the `one_wire_bus` crate traits (or provide a clean `OneWireDevice` endpoint carrying `(OneWireBus, Option<Address>)`) so DS18B20 drivers (`ds18b20` crate or thin adapter) work across both blocking and embassy runtimes without blocking the executor during the 750 ms temperature conversion wait (`ctx.delay.delay_ms(750).await` on embassy).

#### 2.2 Direct Analog Inputs (`<adc>`) and Silicon Pin Capability Maps
1. **Expand `espforge-elements/src/pin_caps.rs`:**
   - Generate/populate per-chip `adc1_pins: &'static [(u8, u8)]` (GPIO → ADC channel), `adc2_pins`, `dac_pins`, `touch_pins` from `esp-metadata-generated`.
   - Enforce `ADC2` + `<wifi>` exclusivity at lowering time (on ESP32/S2/S3/C3, `ADC2` shares analog hardware with Wi-Fi RF and fails at runtime when Wi-Fi is active).
2. **Schema & Lowering (`<adc>` in `# peripherals`, `bus "adc"` in `driver.rs`):**
   - `# peripherals`: `<adc id="adc1" unit="1" attenuation="11dB" />`.
   - Device manifests (`bus "adc"`): synthesizes `pin` (checked against `adc1_pins` for the target chip) and claims both the physical GPIO as analog input and the `(unit, channel)` slot in `ResourceGraph`.
   - Runtime (`espforge-runtime/src/hw/adc.rs`): `AdcChannelStorage` + `AdcChannel` exposing `read_raw() -> u16` and `read_mv() -> u16` (using eFuse calibration when available).

---

### Milestone 3: Multi-Transport Manifests, CS-less SPI, and Shared UART

*Solves G7, G8, G9.*

#### 3.1 Multi-Transport `driver` Manifests (`transport "<kind>"`)
Adopt the additive `transport "<kind>"` design from `research/06-multitransport.md`:
- A `driver "<tag>"` accepts **either** top-level `bus` + `codegen` (existing single-transport form) **or** two or more `transport "<kind>" { bus { ... } codegen { ... } }` blocks (`"i2c"`, `"spi"`, `"uart"`, `"onewire"`, `"none"`).
- `Element.attrs` synthesizes the union of transport-specific attributes (`address`, `cs`, `mode`, `frequency`) as optional on the schema node, while `lower_recipe` resolves `bus="$..."` **first**, selects the matching `DeviceRecipe` for that bus kind, and enforces that transport's required attributes (and rejects attributes belonging to another transport, e.g. `address=` on an SPI `<bme280>`).
- Per-transport `dependency` feature overrides (`transport "i2c" { dependency "..." { features "i2c" } }`) allow dual-bus crates like `bme280` or `mipidsi` to enable only the selected transport's Cargo features.

#### 3.2 CS-less SPI Devices & Struct-Literal Constructors (`ws2812-spi`, `apa102`)
1. **Optional Chip Select in `bus "spi"`:**
   - Support `cs_via "none"` inside `bus "spi" { ... }`:
     - `add_bus_attrs` omits `cs` (and `lower_recipe` rejects `cs=` if written).
     - `SpiTransport` makes `cs: Option<ClaimedPin>`.
     - `bus_emit::spi_device` returns a `SharedSpiBus` / `SpiBusDevice` (wrapping `SpiBus` with `configure(config)` before transactions, implementing `embedded_hal::spi::SpiBus` without toggling a CS pin) instead of `ConfiguredSpiDevice`.
2. **Struct-Literal Constructors (`literal` in `runtime`):**
   - Alongside `constructor "Type::new(...)"`, allow `literal "crate::Type { field: {bus_device}, .. }"` inside `runtime "blocking"` / `"async"`.
   - Validated at schema load by substituting holes with sentinels and parsing as `syn::ExprStruct` (refusing closures, blocks, or unknown holes, and forbidding `{placeholder}` in field-name or struct-path position).

#### 3.3 Shared UART Policy (`bus "uart" { shared #true }`)
- Keep exclusive port ownership as the default for point-to-point UART devices (`mhz19b`).
- Add opt-in `shared #true` on `bus "uart"` backed by a mutex-guarded `UartBus` / `UartBusStorage` (mirroring `I2cBus`), allowing multiple addressable serial devices (e.g. Modbus RTU / RS-485 sensors with a shared `de` / direction-enable GPIO) to share one `<uart>` peripheral safely.

---

### Milestone 4: Constructor, `then`-Step, and Variant Expressiveness

*Solves G10, G12.*

#### 4.1 Richer `then` Steps (Infallible & Typestate Transitions)
Currently, `BusDevice::emit` in `espforge-elements/src/model.rs` appends `.map_err(|_| DeviceError { ... })?;` to every `then` step. Many Rust embedded crates have post-construction methods that either return `()`/`Self` or transform a typestate (`let dev = dev.into_continuous_mode();`).

Extend `then` in `espforge-schema/src/driver.rs`:
- `fallible #false` inside `then { call "set_rotation({rotation})" fallible #false }`: emits `#call #awaited;` with no `.map_err(...)`.
- `rebind #true` (or `transform` / `consume #true`): for methods that consume `self` and return a new type/typestate (`let #local = #local.into_buffered_graphics_mode();` or `let #local = #local.initialize(&mut delay)?;`), avoiding the need for wrapper functions in `espforge-drivers` for simple typestate transitions.

#### 4.2 Generic Arguments in Constructor Paths (`turbofish`)
Relax `path_text` in `espforge-schema/src/driver.rs` so `constructor` paths may include explicit type arguments (e.g., `Driver::<Model>::new(...)` or `Builder::new::<4>(...)`), validating path segments via `syn::Path` rather than rejecting `PathArguments::AngleBracketed`.

#### 4.3 `requires_alloc` in Manifests & Driver Variants (`alias`)
1. **`requires_alloc #true` on `driver` / `dependency`:**
   - Recorded on `Driver` and checked during `options` / `lower`: if the user explicitly turned `<options alloc="false" />` or `<options defaults="false" />` without `alloc="true"`, emit a located diagnostic (`DEVICE_REQUIRES_ALLOC`) instead of letting `cargo build` fail on `alloc::vec` / `esp-alloc`.
2. **Tag `alias` on `driver`:**
   - Allow `alias "sht30" "sht35"` inside `driver "sht31"` (for protocol-identical chip families).
   - `parse_driver` registers aliases in `Schema`, checking global tag uniqueness, and `build.rs` emits matching `pub use` re-exports in `generated`.

#### 4.4 Bounded Recursive `compose`
Replace the 1-level cap in `check_compositions` (`espforge-schema/src/lib.rs`) with cycle detection (DFS over part tags, rejecting `A -> B -> A` with the full cycle in the `SchemaError`), a deterministic depth limit (e.g., `MAX_COMPOSE_DEPTH = 4`), and deterministic hierarchical sub-device IDs (`<parent>_<part>`).

---

### Milestone 5: Latent Codegen & Runtime Bug Fixes

*Solves G8 (clock glitch), G11 (`build.rs` panics).*

1. **Fix SPI Clock-Polarity Glitch Before CS Assertion (`espforge-runtime/src/hw/spi.rs`):**
   - In both `ConfiguredSpiDevice::transaction` and `ConfiguredAsyncSpiDevice::transaction`, call `self.bus.configure(self.config)?;` **before** `self.cs.set_level(self.polarity.asserted());`.
   - Applying `apply_config` (which can change `CPOL`/`CPHA` idle clock state) *while CS is already asserted* clocks a phantom edge into the selected SPI slave.
   - Note: `AsyncSpiBus::configure` also needs to hold the bus lock across both `configure` and the transaction itself in `ConfiguredAsyncSpiDevice::run` so another async task sharing the same SPI bus cannot overwrite `applied` config between `configure()` and `lock().await`! (Currently `ConfiguredAsyncSpiDevice::run` calls `self.bus.configure(self.config).await?`, releases the mutex, and then re-acquires `self.bus.inner.lock().await` — a race condition between two tasks with different SPI modes/frequencies on one bus!)

2. **Fix `espforge-elements/build.rs` and `attrs.rs` Attribute Matrix:**
   - Add `opt_pin_or_ref`, `hex_or`, `opt_number`, `opt_flag` to `espforge-elements/src/attrs.rs`.
   - Handle `(AttrKind::Hex, false, Some(default))`, `(AttrKind::PinOrRef, false, None)`, `(AttrKind::U32, false, None)`, `(AttrKind::Bool, false, None)` in `espforge-elements/build.rs` (`field_type`) instead of panicking.
   - This unblocks `address default="0x3C"` on bundled v2 manifests (`ssd1306.kdl`, `sht31.kdl`) and `PinOrRef` on optional auxiliary pins.

3. **Unify `generated::validate` and `dynamic_attrs::validate` Semantics:**
   - Ensure `AttrKind::List` and range checks behave identically in both `build.rs`-generated `Attrs` and `dynamic_attrs.rs`.

4. **Support `#[embassy_executor::task]` / Interrupt-Driven Input Pins (`irq`):**
   - Allow `pin "..." direction="input" irq=#true` so an interrupt-capable input pin exposes `wait_for_edge()` / `wait_for_low()` safely through the generated `Context` or device handle without forcing users to steal peripherals.

---

## Part 3: How `hx711`, `tm1637`, and `ds18b20` Look Under This Architecture

Once Milestones 1 & 2 land, all three devices are **100% manifest-only** (no `espforge-elements` Rust code needed):

### 1. HX711 Load Cell (`bus "none"`, 2 Pins, `PinOrRef`)
```kdl
driver "hx711" {
    format_version 2
    summary "Avia Semiconductor HX711 24-bit load-cell ADC"
    category "sensor"

    dependency "loadcell" { version "0.2.0" }

    bus "none"

    pin "sck" direction="output" default_level="low"
    pin "dout" direction="input" pull="none"

    codegen {
        map "delay" {
            "runtime" { expr "espforge_runtime::Delay::new()" }
        }
        runtime "blocking" {
            type "loadcell::hx711::HX711<espforge_runtime::OutputPin, espforge_runtime::InputPin, espforge_runtime::Delay>"
            constructor "loadcell::hx711::HX711::new({sck}, {dout}, {delay})"
        }
    }

    attr "delay" kind="choice" default="runtime" { choices "runtime" }
    example "minimal" #"<hx711 id="scale" sck="4" dout="5" />"#
}
```

### 2. TM1637 7-Segment Display (`bus "none"`, Open-Drain / Bidirectional DIO)
```kdl
driver "tm1637" {
    format_version 2
    summary "TM1637 4/6-digit 7-segment LED display"
    category "display"

    dependency "tm1637-gpio-driver" { version "0.3.0" }

    bus "none"

    pin "clk" direction="open_drain" default_level="high"
    pin "dio" direction="open_drain" default_level="high"

    codegen {
        fallible #false
        map "delay" {
            "runtime" { expr "espforge_runtime::Delay::new()" }
        }
        runtime "blocking" {
            type "tm1637_gpio_driver::TM1637<'static, espforge_runtime::OpenDrainPin, espforge_runtime::OpenDrainPin, espforge_runtime::Delay>"
            constructor "tm1637_gpio_driver::TM1637::new({clk}, {dio}, {delay})"
        }
    }

    attr "delay" kind="choice" default="runtime" { choices "runtime" }
    example "minimal" #"<tm1637 id="digits" clk="6" dio="7" />"#
}
```

### 3. DS18B20 Temperature Probe (`bus "onewire"`, Multidrop)
```kdl
driver "ds18b20" {
    format_version 2
    summary "Maxim DS18B20 1-Wire digital thermometer"
    category "sensor"

    dependency "ds18b20" { version "0.1.1" }

    bus "onewire" {
        address optional=#true
    }

    codegen {
        runtime "blocking" {
            type "espforge_runtime::OneWireDevice<ds18b20::Ds18b20>"
            constructor "ds18b20::Ds18b20::new({rom})"
        }
    }

    example "single probe" #"<ds18b20 id="probe" bus="$ow0" />"#
    example "multidrop" #"""
        # espforge
          <project name="probes" />
          <chip type="esp32c3" />
          <runtime type="blocking" />
        # peripherals
          <onewire id="ow0" pin="4" />
        # devices
          <ds18b20 id="indoor"  bus="$ow0" rom="0x28FF112233445501" />
          <ds18b20 id="outdoor" bus="$ow0" rom="0x28FF112233445502" />
        # services
        """#
}
```

---

## Part 4: Verification & Regression Gate

Every milestone must pass the workspace's standing checks with zero unapproved golden churn:

```sh
cargo test --workspace
cargo test -p espforge-elements --test manifests
cargo test -p espforge --test golden_projects -- --ignored
cargo test -p espforge --test generated_projects_compile -- --ignored
cargo test -p espforge --test compile_fail -- --ignored
```

Plus new integration fixtures in `testing/fixtures/` and `espforge/tests/generated_projects_compile.rs` covering:
1. A busless 2-pin driver (`hx711`-shaped) on blocking and embassy boards (both `boot` and `init="deferred"`).
2. An open-drain 2-pin driver (`tm1637`-shaped) verifying `embedded_hal::digital::{InputPin, OutputPin}` on `espforge_runtime::OpenDrainPin` / `Bidirectional`.
3. Two `ds18b20` probes sharing one `<onewire>` bus (with duplicate ROM rejection and pin-conflict rejection).
4. An `<adc>` sensor fixture verifying `ADC2` + `<wifi>` conflict rejection at lowering time.
5. An `AsyncSpiBus` multi-task contention test verifying `configure()` + operations hold the bus mutex atomically after CS configuration ordering is fixed.

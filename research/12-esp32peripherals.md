

### Tier 1 — High Value & Clean Fit for `espforge`

#### 1. **TWAI / CAN Bus (`esp_hal::twai`)**
* **What it unlocks:** Automotive OBD-II readers, robotics motor controllers (ODrive, cybergear), industrial CANopen/DroneCAN nodes.
* **Why it fits `espforge`:**
  * Works across almost the whole ESP32 lineup (`esp32`, `c3`, `c6`, `s2`, `s3`, `h2`, `c5`, `p4`).
  * Matches `<i2c>`/`<uart>` 1-to-1: `<twai id="can0" tx="4" rx="5" baud="500kbps" />` claims `"TWAI0"` (and `TWAI1` on chips that have two) plus two GPIO pins.
  * `esp_hal::twai` supports both blocking and async frame TX/RX and implements `embedded_can`, so it can expose a raw `device!(ctx, can0)` handle and a `bus "twai"` transport for driver manifests.

#### 2. **Hardware Watchdog & Deep-Sleep / RTC Power (`esp_hal::timer::timg::Wdt`, `esp_hal::rtc_cntl`)**
* **What it unlocks:** Unattended IoT sensors that recover from hangs (watchdog) and battery-powered nodes that wake, read a sensor, post over Wi-Fi, and deep-sleep.
* **Why it fits `espforge`:**
  * **Zero extra pins** for WDT (`TIMG1` / `RWDT`—note `lower.rs` already holds `TIMG0.timer0` under Embassy, so using `TIMG1` or `Rtc`'s `Rwdt` avoids colliding with the scheduler).
  * `<wdt id="watchdog" timeout="10s" />` can emit a simple `ctx.watchdog` or `device!(ctx, watchdog).feed()` handle.
  * Deep sleep (`Rtc::sleep_deep` + GPIO/timer wakeup sources) is one of the top reasons people reach for ESP32s over Linux SBCs, and `espforge` can validate at lowering time that any wake pin is actually an RTC-capable GPIO on the target chip.

#### 3. **I2S Audio (`esp_hal::i2s`)**
* **What it unlocks:** I2S microphones (`INMP441`, `ICS-43434`), I2S amplifiers/DACs (`MAX98357A`, `PCM5102`), voice/audio streaming projects.
* **Architectural catch to plan for:** Unlike I2C/SPI/UART today, `esp_hal::i2s::master::I2s` requires a **DMA channel** (`DMA_CH0..`) and static DMA buffers (`dma_buffers!`). Adding `<i2s>` is the point where `ResourceGraph` should gain a shared DMA channel allocator (with `no_static_storage_in_runtime` satisfied by emitting DMA buffer statics in `src/generated.rs`).

#### 4. **Internal Die Temperature Sensor (`esp_hal::tsens`)**
* **What it unlocks:** Chip thermal telemetry (`TemperatureSensor::new(peripherals.TSENS, ...)` -> `to_celsius()`).
* **Why it fits `espforge`:** Tiny implementation surface—zero pins, available on `c2`, `c3`, `c6`, `h2`, `s2`, `s3` (absent on classic `esp32`, gated via `soc_has_temp_sensor`). Note that die temperature reads ~10–15 °C above ambient under load, so it is best documented as board/CPU thermal monitoring rather than an ambient room thermometer.

---

### Tier 2 — Useful, More Specialized

#### 5. **USB Serial/JTAG (`esp_hal::usb_serial_jtag`) & USB OTG (`esp_hal::usb`)**
* **Why consider it:**
  * First, there is a small latent gap in `lower.rs` today: `hold_runtime_resources` holds `UART0` when `console == Console::Uart`, **but does not yet hold `USB_DEVICE`** when `console == Console::JtagSerial`.
  * When `console == Console::Uart`, `USB_SERIAL_JTAG` on `c3`/`c6`/`s3`/`h2` is completely unused and could be exposed as a pinless `embedded_io::Read + Write` / `Async` serial link over the USB cable without touching a UART.
  * **USB OTG** (`esp32s2`, `esp32s3`, `esp32p4`) unlocks USB HID keyboards/mice, CDC ACM, and MIDI via `embassy-usb`, though it is a larger runtime/descriptor integration.

#### 6. **MCPWM (`esp_hal::mcpwm`)**
* **What it unlocks:** Complementary H-bridge gate signals with hardware dead-time, emergency fault trip pins, BLDC motor control, and hardware pulse-width capture.
* **Why Tier 2 instead of Tier 1:** `LEDC` (`ledc.md`) already handles dimming LEDs, buzzers, and RC servos across *all* ESP32 chips including `esp32c3`, whereas `MCPWM` does **not** exist on `esp32c3` or `esp32c2`. Worth adding specifically for motor-control boards on `esp32`/`s3`/`c6`.

#### 7. **Sigma-Delta Modulation (`esp_hal::sdm`)**
* **What it unlocks:** 1-bit PDM/sigma-delta analog-like voltage output and simple audio/LED control on *every* modern ESP32 (replacing the legacy 8-bit DAC).
* **Tradeoff:** Niche compared to `LEDC`; `LEDC` covers 95% of the same use cases with standard `embedded_hal::pwm::SetDutyCycle` traits.

#### 8. **BLE & IEEE 802.15.4 (`esp-radio` / Thread / Zigbee)**
* **What it unlocks:** BLE GATT peripherals/beacons (via `trouble-host` over `esp-radio`) and Zigbee/Thread/Matter on `c6`/`h2`/`c5`.
* **Tradeoff:** These sit on top of `esp-radio` rather than raw `esp-hal` peripherals, similar to `<wifi>` + `<wifi_station>`, and require radio coexistence arbitration when paired with `<wifi>`.

---

### Tier 3 — Probably Not Worth Adding Right Now

| Peripheral | Why skip for now |
|---|---|
| **DAC (`analog::dac`)** | Only exists on classic `esp32` and `esp32s2`; Espressif removed it on C2, C3, C6, H2, and S3. `LEDC` / `SDM` or external I2C DACs (`MCP4725`) are far more portable. |
| **Capacitive Touch (`touch`)** | Only on `esp32`, `s2`, `s3` (absent on C2/C3/C6/H2), and sensitive to PCB parasitic tuning/calibration. |
| **PARL_IO / LCD_CAM** | High-bandwidth DMA parallel bus (`esp32s3`/`c6`/`h2`), mostly used inside custom display/camera drivers rather than as a user-facing `.espf` bus. |
| **AES / SHA / RSA / HMAC / ECC** | Internal crypto accelerators used under the hood by TLS/OTA; rarely configured as `# peripherals` by application authors. |
| **ETM / LP Core** | Low-level silicon event routing and secondary RISC-V ULP firmware loading—better written in custom Rust than modeled in `.espf`. |
| **Native SD/MMC** | `esp-hal` 1.2.2 does not ship an `sdmmc` driver module; SD cards in `esp-hal` currently go over SPI (`embedded-sdmmc`), which `espforge`'s `<spi>` already supports. |

---

### Suggested Next Priority Order (after `rmt`, `pcnt`, `ledc`)

1. **`<twai>`** — clean 2-pin bus, broad chip support, standard `embedded_can` trait, zero DMA needed.
2. **`<wdt>` + `<rtc>` deep sleep** — unlocks reliable & battery-powered IoT devices with minimal schema complexity.
3. **`<i2s>`** — biggest feature unlock (audio), and introduces a **shared DMA channel allocator** (`DMA_CH0..`) in `ResourceGraph` that later helps SPI DMA and USB/parallel peripherals too.
4. **`<tsens>`** + reserving `USB_DEVICE` in `hold_runtime_resources` when `<console type="jtag-serial" />` is active.

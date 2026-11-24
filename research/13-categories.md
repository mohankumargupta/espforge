# `espforge-examples` Category Taxonomy & Numbering

**Purpose:** Canonical directory numbering, naming conventions, and domain boundaries for `espforge-examples/src/examples/<NN.Category>/`.

---

## 1. How `espforge` Reads Categories (Codebase Rules)

Category folders in `espforge-examples/src/examples/` are not hardcoded in Rust; they are discovered at compile time (`espforge-examples/src/lib.rs`):

1. **Zero-padded two-digit prefixes control sort order (`01.`–`99.`).**  
   `CATALOG` sorts categories lexicographically (`a.name.cmp(&b.name)`). Always use `01.`..`09.` before `10.`+ so `10.` does not sort before `02.`.
2. **Use single-word PascalCase names after the dot.**  
   `espforge/src/commands/new.rs` (`category_label`) strips only the leading digits and `.` (for example, `03.Peripherals` becomes `Peripherals` in the interactive wizard). It does **not** replace `_` or `-` with spaces, so single-word nouns (`Environment`, `Motion`, `Light`, `Expanders`) render cleanly without underscores.
3. **Every example short name must be globally unique.**  
   `espforge-examples/src/lib.rs` tests (`every_short_name_resolves_to_its_own_example`) require that `<example>` resolves unambiguously across all categories for `espforge new --example <name>` and `espforge gen --example <name>`. Prefix or qualify names if needed (for example, `dht22`, `ds18b20_multi`, `pcf8574_gpio`).
4. **Update `README.md` when adding categories.**  
   `espforge examples` updates automatically from `EXAMPLES_DIR`, whereas the summary table in `README.md` is maintained manually.

---

## 2. Full Category List (`01`–`16`)

### Foundation & Core Hardware (`01`–`06`)

| # | Directory Name | Wizard Label | Scope & What Belongs Here | Current & Planned Examples |
|---|---|---|---|---|
| **01** | `01.Basics` | `Basics` | Empty skeletons, logging, minimal boot, single-LED blink loops. | **Existing:** `minimal`, `helloworld`, `blink`, `blink_async` |
| **02** | `02.Digital` | `Digital` | Discrete GPIO input/output, push buttons, switches, relays, interrupts, and cross-task button signals. | **Existing:** `button`, `button_async`<br>**Planned:** `relay`, `button_debounce` |
| **03** | `03.Peripherals` | `Peripherals` | **On-chip ESP32 silicon blocks** used directly rather than via an external sensor driver: `RMT`, `PCNT`, `LEDC` (PWM), `TWAI` (CAN), `ADC`, `I2S`, `WDT`, `RTC` (deep sleep), `TSENS` (die temp), `MCPWM`, `USB`. | **Existing (migrate from `04`):** `rmt_tx`, `rmt_tx_async`<br>**Planned:** `rmt_rx`, `pcnt_counter`, `pcnt_encoder`, `ledc_fade`, `ledc_fade_async`, `adc_oneshot`, `twai_loopback`, `wdt_watchdog`, `rtc_deep_sleep`, `tsens_die_temp` |
| **04** | `04.Communication` | `Communication` | Raw serial/fieldbus protocols, bus scanners, multi-device bus sharing, and custom Wokwi chip wiring (`I2C`, `SPI`, `UART`, `1-Wire`, `RS-485`/`Modbus`). | **Existing:** `i2c_example`, `i2c_example_async`, `i2c_multi`, `i2c_scanner`, `spi_async`, `spi_example`, `spi_example_async`, `spi_multi`, `uart_example`, `uart_example_async`<br>**Planned:** `onewire_scanner`, `rs485_modbus` |
| **05** | `05.Networking` | `Networking` | IP networking over Wi-Fi/Ethernet: DHCP, HTTP/HTTPS, WebSockets, NTP wall clock, OTA updates, MQTT. | **Existing:** `wifi_station`, `http_client`, `https_client`, `ws_client`, `wss_client`, `ntp`, `ota_update`<br>**Planned:** `mqtt_client` |
| **06** | `06.Displays` | `Displays` | Visual output devices: OLEDs, TFTs, character LCDs, 7-segment/LED matrices, addressable RGB strips, and e-paper panels. | **Existing:** `ili9341`, `ssd1306`, `ssd1306_async`, `ssd1306_wifi`, `ws2812`, `ws2812_async`<br>**Planned:** `tm1637`, `hd44780`, `aip31068`, `max7219`, `ht16k33`, `epd_waveshare` |

### IoT Sensors, Expanders & Subsystems (`07`–`16`)

| # | Directory Name | Wizard Label | Scope & What Belongs Here | Current Manifests & Planned Examples |
|---|---|---|---|---|
| **07** | `07.Environment` | `Environment` | Temperature, humidity, barometric pressure, CO₂, VOC, and air-quality sensors. | **Manifests ready:** `sht31`, `sht4x`, `bme680`, `dht22`, `scd4x`, `sgp30`, `mhz19b`<br>**Planned:** `bme280`, `aht20`, `ccs811`, `sen6x`, `ds18b20` |
| **08** | `08.Motion` | `Motion` | Inertial Measurement Units (accelerometers, gyroscopes), magnetometers, tilt/vibration, and external quadrature/position sensors. *(Alternative name: `08.IMU` if restricted to IMUs.)* | **Manifests ready:** `adxl345`<br>**Planned:** `bmi323`, `icm20948`, `mpu9250`, `tlv493d` |
| **09** | `09.Light` | `Light` | Ambient lux, UV, color/spectral, optical proximity, and laser Time-of-Flight sensors. *(Alternative name: `09.Optical`.)* | **Manifests ready:** `bh1750`<br>**Planned:** `as7262`, `veml7700`, `vl53l0x`, `apds9960` |
| **10** | `10.Expanders` | `Expanders` | Digital I/O expanders, shift registers, I²C bus multiplexers, and external PWM/driver ICs. *(Matches `category "expander"` in `nxp74hc595.kdl`.)* | **Manifests ready:** `nxp74hc595`<br>**Planned:** `pcf8574`, `mcp23017`, `tca9548a`, `pca9685` |
| **11** | `11.Analog` | `Analog` | External ADCs/DACs, load-cell amplifiers, voltage/current/power monitors, and analog sensor probes (thermistors, potentiometers, soil moisture). | **Manifests ready:** `ads111x`, `hx711`<br>**Planned:** `ina219`, `ads1015`, `ntc_thermistor`, `axp2101` |
| **12** | `12.Actuators` | `Actuators` | Physical motion and electromechanical outputs: RC servos, DC/H-bridge motors, stepper drivers, solenoids, and piezo buzzers. | **Planned:** `servo_sweep`, `stepper_motor`, `dc_motor_pwm`, `buzzer_tone` |
| **13** | `13.Audio` | `Audio` | I²S microphones, audio amplifiers/DACs, tone generation, and PCM/PDM streaming. | **Planned:** `i2s_inmp441_mic`, `i2s_max98357a_amp` |
| **14** | `14.Time` | `Time` | Offline wall-clock timekeeping and positioning: external battery-backed RTC chips and GNSS/GPS receivers. *(Matches `category "time"` in `ds3231.kdl`.)* | **Manifests ready:** `ds3231`<br>**Planned:** `gps_nmea` |
| **15** | `15.Storage` | `Storage` | Persistent external storage and memory: microSD cards (SPI/FAT), external EEPROM, FRAM, and flash logging. | **Planned:** `sd_card_spi`, `eeprom_at24c` |
| **16** | `16.Identification` | `Identification` | Identity, access control, and hardware security: RFID/NFC readers, barcode/UART scanners, and secure elements. | **Planned:** `mfrc522_rfid` |

*(Optional future tier if non-IP RF is separated from `05.Networking`: **17.Wireless** for LoRa (`lora-phy`), BLE beacons, ESP-NOW, and Thread/Zigbee.)*

---

## 3. Complementary Naming & Division Guidelines

Because embedded systems often blur "bus", "peripheral", and "device", use these boundary rules so every new example has one obvious home:

### 1. `03.Peripherals` vs. `04.Communication`
* **`03.Peripherals`** demonstrates **on-chip silicon engines** configured and driven directly without an external protocol bus (`RMT` pulse trains, `PCNT` hardware counters, `LEDC` PWM timers, `ADC` raw sampling, `WDT` watchdog, `RTC` sleep, `TSENS` die temperature).
  * *Migration note:* `rmt_tx` and `rmt_tx_async` currently reside in `04.Communication`; moving them to `03.Peripherals` keeps `04.Communication` strictly focused on serial communication buses (`I2C`, `SPI`, `UART`, `1-Wire`), while noting that `research/12-ledc.md` (`03.Analog`) and `research/12-pcnt.md` (`03.Counters`) are superseded by `03.Peripherals`.
* **`04.Communication`** demonstrates **bus protocols and multi-peer wiring** (`i2c_scanner`, `i2c_multi`, `spi_multi`, `uart_example`). If an example demonstrates how `espforge` shares one bus across different speeds/modes or talks to a custom Wokwi chip, put it here.

### 2. `06.Displays` vs. `09.Light`
* **`06.Displays`** = **light emission / visual UI** (OLEDs, TFTs, 7-segment `tm1637`, character LCDs, NeoPixel/WS2812 strips).
* **`09.Light`** = **light sensing / optical measurement** (lux meters like `bh1750`, color/spectral `as7262`, UV, optical ToF `vl53l0x`).  
  *(If "Light" feels ambiguous with LEDs/lamps, name this `09.Optical` instead.)*

### 3. `03.Peripherals` vs. `07.Environment` / `11.Analog` / `12.Actuators`
* Reading raw counts from on-chip `<adc>` belongs in **`03.Peripherals`** (`adc_oneshot`). Turning an analog pin or an external I²C ADC (`ads111x`, `hx711`, `ina219`) into a calibrated physical measurement belongs in **`11.Analog`** (or **`07.Environment`** if it is an environmental probe like an NTC thermistor).
* Generating a raw PWM signal with `<ledc>` belongs in **`03.Peripherals`** (`ledc_fade`); driving a physical servo angle or stepper sequence belongs in **`12.Actuators`**.

### 4. `08.Motion` vs. `08.IMU`
* **`08.Motion`** is recommended as the broader complement to `07.Environment` and `09.Light`: it holds 3/6/9-axis IMUs (`adxl345`, `bmi323`, `icm20948`, `mpu9250`), Hall/magnetic sensors (`tlv493d`), and PIR/radar motion detectors.
* If you prefer a narrower category strictly for inertial chips, use **`08.IMU`** (and place rotary/flow pulse counting in `03.Peripherals` or `02.Digital`).

### 5. `05.Networking` vs. `14.Time`
* **`05.Networking`** owns IP protocols and connected services (`ntp` over Wi-Fi, `http_client`, `ota_update`).
* **`14.Time`** owns offline hardware clocks (`ds3231`) and satellite/RF timing (`gps_nmea`).

### 6. `02.Digital` vs. `10.Expanders` vs. `12.Actuators`
* **`02.Digital`** = MCU-direct pins (buttons, simple LEDs, basic relays).
* **`10.Expanders`** = chips that multiply pins or buses (`nxp74hc595`, `pcf8574`, `mcp23017`, `pca9685`, `tca9548a`).
* **`12.Actuators`** = mechanical or acoustic loads (servos, steppers, motors, buzzers), even when driven through an expander like `pca9685`.

---

## 4. Mapping Existing `espforge-drivers/manifests/*.kdl` to Categories

| Bundled Manifest | Current Manifest `category` | Recommended `espforge-examples` Category |
|---|---|---|
| `ssd1306.kdl`, `ili9341.kdl`, `tm1637.kdl`, `ws2812_rmt.kdl` | `display` | `06.Displays` *(with `spi_multi` / `i2c_multi` remaining in `04.Communication`)* |
| `sht31.kdl`, `sht4x.kdl`, `bme680.kdl`, `dht22.kdl`, `scd4x.kdl`, `sgp30.kdl`, `mhz19b.kdl` | `sensor` | `07.Environment` |
| `adxl345.kdl` | `sensor` | `08.Motion` *(or `08.IMU`)* |
| `bh1750.kdl` | `sensor` | `09.Light` *(or `09.Optical`)* |
| `nxp74hc595.kdl` | `expander` | `10.Expanders` |
| `ads111x.kdl`, `hx711.kdl` | `sensor` | `11.Analog` |
| `ds3231.kdl` | `time` | `14.Time` |

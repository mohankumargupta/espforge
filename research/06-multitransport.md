# `multibus.md`: Multi-Transport Support for `format_version 2` Driver Manifests

**Baseline:** `espforge` workspace `0.9.0`, branch `dev`, with `format_version 2`
`driver` manifests, `driverfixes.md` capabilities (`fallible #false`, fixed
constructor arguments, generic `endpoint`s, per-mode dependency features), and
single-bus `DeviceRecipe` lowering.

**Purpose:** allow a single `driver "<tag>"` manifest to support multiple
hardware bus transports (for example `<bme280 bus="$i2c0" address="0x76" />` and
`<bme280 bus="$spi2" cs="10" />`) without splitting tags into `<bme280_i2c>` /
`<bme280_spi>` or re-introducing per-chip Rust code into `espforge-elements` /
`espforge-drivers`.

**Explicitly not in this document:** PWM/ADC/RMT/I2S/OneWire buses, device
`provides` (I²C/SPI expanders and multiplexers), CS-less SPI devices, struct
literal constructors, and multi-bus syntax for deprecated v1 `element`
manifests (`espforge driver migrate` stays single-bus).

---

## 0. Motivation & Physical Hardware Reality

Many sensor and display ICs expose multiple buses on the **same physical
silicon and breakout PCB** (for example SparkFun BME280 SEN-13676, Adafruit
BME280 #2652, Adafruit BME680 #3660, Adafruit ADXL345 #1231, and 6-pin
GY-BME280 boards), selecting I²C vs. SPI purely by how the `CS` line is wired.
Rust crates such as `bme280` (`0.5.1`) and `bme280-multibus` (`0.5.0`) support
both transports from one dependency.

Today in `espforge`, two dual-bus chips are already forced into one bus each
(`espforge-drivers/manifests/bme680.kdl` is I²C-only; `adxl345.kdl` is
SPI-only) because the manifest model is strictly 1-to-1 with a single bus.

---

## 1. Codebase Audit: Where Single-Bus Assumptions Live

Measured against the current workspace:

| Layer | File | Current Single-Bus Constraint | Impact of Multi-Transport |
|---|---|---|---|
| Schema IR | `espforge-schema/src/lib.rs` | `DeviceRecipe` has one `bus: String`, one `ty`, one `new`, one `endpoint`, one `address`, one `spi_config`, one `args`. `Element` has `device_recipe: Option<DeviceRecipe>`. | Must hold 1..N transport recipes (one per bus kind) while keeping single-bus ergonomics for existing callers. |
| Schema v2 Parser | `espforge-schema/src/driver.rs` | `parse_driver` forbids more than one `bus` or `codegen` block; `add_bus_attrs` synthesizes attributes for one bus kind; `build_recipe` builds one `DeviceRecipe`. | Add `transport "<kind>" { ... }` blocks (mutually exclusive with top-level `bus` + `codegen`), synthesize union attributes on `Element`, validate each transport independently. |
| Composition check | `espforge-schema/src/lib.rs` (`check_compositions`) | Compares `part_recipe.bus != recipe.bus`. | A composite transport's bus must match a supported transport on the composed part. |
| Attr Coercion | `espforge-elements/src/attrs.rs` | Has `pin_or_ref` (required) and `opt_hex` (optional), **lacks `opt_pin_or_ref` and `hex_or`**. | Must add `opt_pin_or_ref` (so `cs` can be optional in static `Attrs` and checked per-transport at lowering) and `hex_or`. |
| Static `Attrs` Gen | `espforge-elements/build.rs` | `field_type` panics on `(AttrKind::PinOrRef, false, None)` and `(AttrKind::Hex, false, Some(_))`. | Add both arms so `build.rs` can generate `Attrs` for multi-bus manifests (and single-bus `address default="..."` drivers). |
| Runtime `Attrs` Gen | `espforge-elements/src/dynamic_attrs.rs` | Checks `element.attrs` generically; skips absent optional attributes. | Works for presence; lowering handles transport-specific required/forbidden checks. |
| Lowering | `espforge-elements/src/devices/recipe.rs` | `lower_recipe` checks `kind != recipe.bus`, parses `address`/`cs`/`spi_config` from `element.attrs`. | Must resolve `bus="$..."` *first*, select the matching transport recipe, enforce transport-specific required & forbidden attributes, and read transport defaults from the selected transport. |
| Board IR | `espforge-elements/src/model.rs` | `BusDevice` holds one concrete `Driver` and one `BusTransport` (`I2c \| Spi \| Uart`). | **No structural change needed** — `Board` is already post-lowering monomorphized to the chosen bus. |
| Lowering Checks | `espforge-elements/src/lower.rs` | `check_i2c_addresses`, `check_bus_modes`, `check_exclusive_driver_modes` inspect `BusDevice`. | **No change needed** — runs on resolved `BusDevice`s. |
| Code Emission | `espforge-emit/src/codegen/*` | Reads `Board`, `BusDevice`, `Driver`, `BusTransport`. | **No change needed** (so long as `Driver.dependencies` carries only the chosen transport's dependency adjustments). |
| Migration & Lock | `espforge-core/src/drivers.rs` | `migrate_element` converts v1 → v2 (`device_recipe`); lockfile hashes manifest bytes. | Lockfile needs no change; `migrate_element` continues emitting single-bus v2 syntax. |

**Key architectural property:** Everything downstream of `lower_recipe`
(`model.rs`, `lower.rs`, `bus_emit.rs`, `espforge-emit`) already operates on
a single resolved `(Driver, BusTransport)`. Multi-transport is purely a
**schema + attr generation + lowering** feature.

---

## 2. KDL v2 Manifest Design

### 2.1 Grammar: Additive `transport "<kind>"` Blocks

A `driver "<tag>"` manifest supports **either**:

1. **Single-transport form (existing, unchanged):** top-level `bus "<kind>"`
   and top-level `codegen { ... }`. Every existing manifest (`sht31.kdl`,
   `ssd1306.kdl`, `adxl345.kdl`, `mhz19b.kdl`, etc.) stays valid without edits.
2. **Multi-transport form (new):** two or more `transport "<kind>" { ... }`
   blocks (`"i2c"`, `"spi"`, or `"uart"`), with **no** top-level `bus` or
   top-level `codegen` (except an optional shared `map` / `fallible` rule—see
   §2.3).

```kdl
driver "bme280" {
    format_version 2
    summary "Bosch BME280 temperature, humidity and pressure sensor"
    description "Reads temperature, humidity and pressure over I2C or SPI."
    category "sensor"

    dependency "bme280" { version "0.5.1" }

    transport "i2c" {
        bus {
            address default="0x76" allowed="0x76..=0x77"
        }
        codegen {
            fallible #false
            map "delay" {
                "runtime" { expr "espforge_runtime::Delay::new()" }
            }
            runtime "blocking" {
                type "bme280::i2c::BME280<espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>>"
                constructor "bme280::i2c::BME280::new({bus_device}, {address}, {delay})"
                endpoint "espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>"
            }
        }
    }

    transport "spi" {
        bus {
            mode 0
            frequency "10000kHz"
        }
        codegen {
            fallible #false
            map "delay" {
                "runtime" { expr "espforge_runtime::Delay::new()" }
            }
            runtime "blocking" {
                type "bme280::spi::BME280<espforge_runtime::ConfiguredSpiDevice>"
                constructor "bme280::spi::BME280::new({bus_device}, {delay})"
                endpoint "espforge_runtime::ConfiguredSpiDevice"
            }
        }
    }

    attr "delay" kind="choice" default="runtime" {
        choices "runtime"
        help "The delay handle the driver takes; espforge always passes the runtime's own."
    }
    attr "init" kind="choice" default="boot" {
        choices "boot" "deferred"
        help "`boot` builds the sensor during `Devices::new`; `deferred` stores the bus and constructs it in `init()`."
    }

    example "i2c" #"<bme280 id="env" bus="$i2c0" address="0x76" />"#
    example "spi" #"<bme280 id="env" bus="$spi2" cs="10" />"#
    see_also "peripherals/i2c"
    see_also "peripherals/spi"
}

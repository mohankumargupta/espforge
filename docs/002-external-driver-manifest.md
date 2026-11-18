The `codegen` block is the **Rust code template** for your driver. It tells `espforge` exactly what Rust struct type and constructor call to write into `src/generated.rs` when a user puts your `<tag ... />` in their `.espf` file.

Everything else in the `.kdl` file describes the **ESPF side** (attributes, bus kind, dependencies). `codegen` bridges that to the **Rust side**.

---

### The easiest way to see it: Input → Generated Output

Suppose your `sht31.kdl` has this `codegen` block:

```kdl
codegen {
    map "delay" {
        "runtime" { expr "espforge_runtime::Delay::new()" }
    }
    runtime "blocking" {
        type "embedded_sht3x::Sht3x<espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>, espforge_runtime::Delay>"
        constructor "embedded_sht3x::Sht3x::new({bus_device}, {address}, {delay})"
        endpoint "espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>"
    }
}
```

When a user writes:
```xml
<sht31 id="air" bus="$i2c0" address="0x44" />
```

`espforge` plugs those values into `codegen` and generates this in `src/generated.rs`:

```rust
pub struct Devices {
    // Filled from `type "..."`
    pub air: embedded_sht3x::Sht3x<
        espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<
            'static,
            esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>
        >,
        espforge_runtime::Delay,
    >,
}

impl Devices {
    pub fn new(registry: &mut PeripheralRegistry) -> Result<Self, espforge_runtime::DeviceError> {
        let i2c0_bus = ...; // built automatically from `bus "$i2c0"`

        Ok(Self {
            // Filled from `constructor "..."`:
            //   {bus_device} -> CriticalSectionDevice::new(i2c0_bus.cs_cell())
            //   {address}    -> 68  (0x44)
            //   {delay}      -> espforge_runtime::Delay::new()   (from `map "delay"`)
            air: embedded_sht3x::Sht3x::new(
                espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice::new(i2c0_bus.cs_cell()),
                68,
                espforge_runtime::Delay::new(),
            ).map_err(|_| espforge_runtime::DeviceError {
                device: "air",
                kind: espforge_runtime::DeviceErrorKind::Init,
            })?,
        })
    }
}
```

---

### Every part of `codegen` explained

#### 1. `runtime "blocking" { ... }` and `runtime "async" { ... }`
A driver supports blocking mode, async mode, or both (one block for each). Inside a `runtime` block there are up to 5 keys:

* **`type "..."`** *(required)*  
  The Rust type of the field stored in `pub struct Devices`.  
  *(You have to spell out the concrete generic parameters like `<CriticalSectionDevice<...>, Delay>` because Rust struct fields cannot use `impl Trait` or `_`.)*

* **`constructor "..."`** *(required)*  
  The function call that builds the driver. It must be a plain function/method call (`Crate::Type::new(...)`), and its **first argument must always be `{bus_device}`** (or `{spi_device}`).

* **`endpoint "..."`** *(optional, required only if `init="deferred"` or `runtime "async"` is used)*  
  When a user sets `init="deferred"` (or uses an async driver, which is always deferred), `Devices::new` doesn't construct the driver at boot—it creates a `Factory` struct holding the bus handle until the user calls `.init()` in `app.rs`. `endpoint` is simply the **Rust type of `{bus_device}`** so `espforge` knows what type to put inside that `Factory` struct:
  ```rust
  pub struct AirFactory {
      endpoint: /* whatever `endpoint "..."` says */,
  }
  ```
  * On blocking I²C, `{bus_device}` is `espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>`.
  * On async I²C, `{bus_device}` is `espforge_runtime::AsyncI2cBus`.
  * On blocking SPI, `{bus_device}` is `espforge_runtime::ConfiguredSpiDevice`.
  * On async SPI, `{bus_device}` is `espforge_runtime::ConfiguredAsyncSpiDevice`.
  * On UART, `{bus_device}` is `espforge_runtime::UartDevice<esp_hal::Blocking>` (or `Async`).

* **`awaits #false`** *(only inside `runtime "async"`, optional)*  
  By default, an async factory calls `constructor(...).await`. If the crate's async `new()` is a regular synchronous `fn` (like `edrv_mhz19::MHZ19::new` in `mhz19b.kdl`), `awaits #false` tells `espforge` not to emit `.await`.

* **`feature "..."`** *(only inside `runtime "async"`, optional)*  
  Overrides the Cargo feature on `espforge-drivers` when running in async mode (e.g., `ssd1306-async` instead of `ssd1306`).

---

#### 2. `map "<attr_name>" { ... }`
ESPF attributes are strings (`size="128x64"`, `rotation="90"`), whereas Rust wants types or enum variants. `map` translates each allowed ESPF string into a Rust `type` or `expr` (`ssd1306.kdl`):

```kdl
map "size" {
    "128x64" { type "espforge_drivers::Oled128x64" }
    "128x32" { type "espforge_drivers::Oled128x32" }
}
map "rotation" {
    "0"   { expr "espforge_drivers::DisplayRotation::Rotate0" }
    "90"  { expr "espforge_drivers::DisplayRotation::Rotate90" }
    ...
}
```
Then `{size}` can be substituted into both `type "Ssd1306Display<{size}>"` and `constructor "...({size}, {rotation})"`.

*(Clever trick used in `sht31.kdl` / `bme680.kdl`: if a driver needs `espforge_runtime::Delay::new()` as an argument, you can declare `attr "delay" kind="choice" default="runtime" { choices "runtime" }` and map `"runtime" { expr "espforge_runtime::Delay::new()" }` so `{delay}` injects the delay automatically without the user ever writing it in `.espf`.)*

---

#### 3. `{placeholders}` vs fixed values in `constructor`
Inside `constructor "..."`, `espforge` replaces `{holes}` from a closed set checked at load time with `syn`:
* `{bus_device}` (or `{spi_device}`): the bus endpoint (always 1st arg).
* `{address}`: the I²C 7-bit address as a number (must be 2nd arg if used).
* `{dc_pin}` or `{dc}`: a claimed `pin "..."` converted into an `Output`, `Input`, or `Bidirectional` pin.
* `{any_mapped_attr}`: replaced by whatever `map "any_mapped_attr"` resolves to.
* **Literal/fixed Rust expressions** (no braces!): anything else is passed verbatim to Rust—for example `&bosch_bme680::Configuration::default()` and `20` in `bme680.kdl`.

---

#### 4. `fallible #false` *(optional, defaults to `#true`)*
* Omit it if `constructor` returns `Result<Self, E>` (appends `.map_err(...)?`).
* Write `fallible #false` if `constructor` returns `Self` directly without a `Result` (like `sht4x.kdl` and `mhz19b.kdl`).

---

#### 5. `feature "..."` *(optional, bundled drivers only)*
* Only used by drivers that live inside the `espforge-drivers` crate (like `ssd1306.kdl`).
* **Omit it** for manifest-only drivers that use a crates.io crate via `dependency "..."` directly (`sht31.kdl`, `bme680.kdl`, `ads111x.kdl`, `sht4x.kdl`, `mhz19b.kdl`).

---

### Why is `codegen` checked so strictly?
Because a KDL manifest is generating Rust code! `espforge-schema/src/driver.rs` parses `type`, `constructor`, `endpoint`, and every `map` entry through `syn` when loading the `.kdl` file—so if you make a typo in a Rust type or write an unknown `{placeholder}`, `espforge` catches it immediately when reading the manifest rather than spitting out broken Rust in `src/generated.rs`.

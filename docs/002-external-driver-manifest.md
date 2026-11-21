# External driver manifests (`.kdl`)

A driver manifest describes one `<tag />`: the arguments a user may write, the
bus it binds to, and the Rust type and constructor `espforge` emits into
`src/generated.rs`. Bundled manifests live in `espforge-drivers/manifests/`; a
project may also supply its own. Every manifest is `format_version 2`.

The file has two halves. The **ESPF side** (`bus`, `pin`, `attr`, `dependency`)
says what the element looks like and what it needs. The **Rust side**
(`codegen`) says what to build. Only `codegen` writes Rust.

---

## 1. Skeleton

```kdl
// Illustrative: one manifest showing every node.
driver "demo" {
    format_version 2
    summary "A demo sensor"
    description "Reads a value over an I2C bus."
    category "sensor"

    dependency "embedded-sht3x" { version "0.2.0" }

    bus "i2c" {
        address allowed="0x44..=0x45"
    }

    pin "alert" direction="input" pull="up" optional=#true

    codegen {
        map "delay" {
            "runtime" { expr "espforge_runtime::Delay::new()" }
        }
        runtime "blocking" {
            type "..."
            constructor "..."
            endpoint "..."
        }
    }

    attr "init" kind="choice" default="boot" {
        choices "boot" "deferred"
    }
    attr "delay" kind="choice" default="runtime" {
        choices "runtime"
    }

    example "minimal" #"<demo id="air" bus="$i2c0" address="0x44" />"#
    see_also "peripherals/i2c"
}
```

| Node | Required | Meaning |
|---|---|---|
| `format_version 2` | yes | The only accepted version. |
| `summary "..."` | yes | One line shown by `espforge examples` and help. |
| `description "..."` | no | Longer prose for help and docs. |
| `category "..."` | no | Defaults to `device`. Used as the driver's kind in diagnostics (`async display needs deferred init`). |
| `bus "..."` | one transport | `i2c`, `spi`, `uart` or `none`. |
| `transport "..."` | instead of `bus` | Two or more bus kinds for one tag; see §7. |
| `pin "..."` | no | A line the driver owns. At least one for `bus "none"`. |
| `attr "..."` | no | An extra element attribute. |
| `compose "..."` | no | A companion driver on the same bus; see §8. |
| `codegen { ... }` | yes | The Rust type, constructor and maps. |
| `dependency "..."` | no | A crate the manifest-only driver needs. |
| `example "..."` | yes, at least one | Title plus an `.espf` fragment. |
| `see_also "..."` | no | A `section/leaf` target such as `peripherals/i2c`. |

Unknown nodes, duplicate nodes and duplicate names are load errors. The
element always gets an `id` attribute; a bus device also gets a required `bus`
attribute, and the bus adds `address`, `cs`, `mode` and `frequency` as
described below.

---

## 2. `bus`: what the element binds to

```kdl
bus "i2c" { address allowed="0x48..=0x4B" }
bus "spi" {
    mode 0
    frequency "40000kHz"
    cs_polarity "active-low"
    cs_via "pin_or_ref"
}
bus "uart"
bus "none"
```

### `bus "i2c"`

One optional `address` child, with exactly one of:

| Form | Element attribute | Check |
|---|---|---|
| `address fixed="0x76"` | required | must equal `0x76`. |
| `address default="0x3C"` | optional, defaults to `0x3C` | must be a usable address. |
| `address allowed="0x08..=0x77"` | required (unless a `default` is also given) | must fall in the inclusive range. |
| `address` (bare) | required | any usable 7-bit address, `0x08..=0x77`. |
| no `address` child | none | the manifest may not use `{address}`. |

`fixed` and `default` are mutually exclusive, and a `fixed` address must sit
inside `allowed` when both are written.

### `bus "spi"`

| Child | Meaning |
|---|---|
| `mode 0` | SPI mode 0-3, applied before each transaction. |
| `frequency "40000kHz"` | Clock, applied before each transaction. |
| `cs_polarity "active-low"` | `active-low` (default) or `active-high`. |
| `cs_via "pin_or_ref"` | How `cs=` is accepted. `pin_or_ref` is the only form today. |

The element always gets a required `cs` attribute (a bare GPIO number or a
`$<gpio>` reference). Declaring `mode` or `frequency` also gives the element
optional `mode` and `frequency` attributes, defaulting to `0` and
`40000kHz`; the device applies them before every transfer, which is what lets
two parts with different modes share one controller. Without those children the
project's bus configuration is left alone. There is no CS-less SPI form yet.

### `bus "uart"`

Takes no children. The device owns the port; a second device on the same
`<uart>` is refused.

### `bus "none"`: busless, GPIO-timed drivers

A busless driver consumes no peripheral. It owns the pins it declares, and
`espforge` never builds a bus handle for it.

```kdl
driver "hx711" {
    format_version 2
    summary "HX711 24-bit load-cell amplifier"
    category "sensor"

    dependency "loadcell" { version "0.3.0" }

    bus "none"

    pin "sck" direction="output" default_level="low"
    pin "dout" direction="input" pull="none"

    codegen {
        fallible #false
        map "delay" { "runtime" { expr "espforge_runtime::Delay::new()" } }
        runtime "blocking" {
            type "loadcell::hx711::HX711<espforge_runtime::OutputPin, espforge_runtime::InputPin, espforge_runtime::Delay>"
            constructor "loadcell::hx711::HX711::new({sck}, {dout}, {delay})"
        }
    }

    attr "delay" kind="choice" default="runtime" { choices "runtime" }
    example "minimal" #"<hx711 id="scale" sck="4" dout="5" />"#
}
```

The rules:

- `bus "none"` takes no children.
- The driver must declare at least one `pin`; a busless driver is its pins.
- The element gets **no** `bus` attribute. Writing `bus="$i2c0"` is an unknown
  attribute.
- The constructor takes no bus: `{bus_device}`, `{spi_device}`, `{address}` and
  `{cs_pin}` are all refused. Its arguments are the declared pins, mapped
  attributes and fixed expressions, in any order.
- An async-only driver cannot be busless (`async driver without a bus`).
- `init="deferred"` is allowed and needs no `endpoint`: the factory stores the
  raw `esp_hal::peripherals::GPIO<n><'static>` values and rebuilds each line in
  `init()`.
- Lowering produces `Device::Pins`, not a `BusDevice`, so no `bus_mode` lookup
  is ever made for it. On an embassy board the field is still wrapped in
  `AsyncMutex`, exactly like any other device.

---

## 3. `pin`: the lines a driver owns

```kdl
pin "dc" direction="output" default_level="low"
pin "busy" direction="input" pull="up"
pin "dio" direction="open_drain" pull="up" default_level="high"
pin "backlight" direction="output" default_level="high" optional=#true
```

| Property | Values | Default | Notes |
|---|---|---|---|
| `direction` | `output`, `input`, `bidirectional`, `open_drain` | `output` | `open-drain` is accepted as a spelling. |
| `default_level` | `low`, `high` | `low` | Only for `output` and `open_drain`. |
| `pull` | `up`, `down`, `none` | `none` | Not on an `output`. |
| `optional` | `#true`, `#false` | `#false` | An absent optional pin is not claimed. |
| `via` | `ref`, `pin_or_ref` | `pin_or_ref` | `ref` restores the strict `$<gpio>` form. |

Each pin becomes an element attribute. By default it is `PinOrRef`: the user
writes a bare GPIO number (`clk="6"`) or a `$<gpio>` reference
(`clk="$gpio6"`); a reference is also checked against the direction the
`<gpio>` declared, and an optional pin may be omitted entirely. `via="ref"`
synthesizes a plain reference instead, for a driver that always wants that
direction check.

The concrete Rust type `{bus}`-style pin holes receive:

| Direction | Rust type |
|---|---|
| `output` | `esp_hal::gpio::Output<'static>` (`espforge_runtime::OutputPin`) |
| `input` | `esp_hal::gpio::Input<'static>` (`espforge_runtime::InputPin`) |
| `bidirectional` | `espforge_runtime::Bidirectional` |
| `open_drain` | `espforge_runtime::OpenDrainPin` |

`OpenDrainPin` drives low or releases, never drives high: `set_high` releases
the line so an external pull-up sets it high. That is the electrical contract
`tm1637`, `dht11`/`dht22` and 1-Wire share, and it implements
`embedded_hal::digital::{InputPin, OutputPin}` so those crates accept it
directly. The wrappers are re-exported as `espforge_runtime::gpio::{Flex,
Input, Output}`, so a manifest never has to spell `esp_hal::gpio::..` itself.

---

## 4. `attr`: extra element attributes

```kdl
attr "size" kind="choice" default="128x64" {
    choices "128x64" "128x32"
    help "Pixel dimensions of the OLED panel."
}
attr "count" kind="u32" default="1" range="1..=4"
attr "reset" kind="pin"
attr "mode" kind="choice" required=#true { choices "fast" "slow" }
```

| `kind` | Element value | Useful properties |
|---|---|---|
| `ident` | a Rust-style identifier | |
| `str` | free text | |
| `pin` | GPIO number | |
| `pin_or_ref` | GPIO number or `$<gpio>` | |
| `freq` | `"40000kHz"` | |
| `hex` | a byte, `0x44` | |
| `u32` | unsigned integer | `range="low..=high"` |
| `bool` | `true` / `false` | |
| `choice` | one of the listed choices | child `choices` |
| `ref` | `$<name>` of the kind in `to=` | `to="gpio"` |
| `list` | repeated values of the inner kind | `of="str"` |

Properties: `required=#true` (default `#false`), `default="..."`,
`range="low..=high"`, `to=` for `ref`, `of=` for `list`; child nodes
`choices "..."` and `help "..."`.

An attribute that appears as a `{hole}` in `codegen` needs a matching `map`
(§5), unless it is a pin or one of the bus holes. That check happens at load
time, so a missing map is a schema error rather than generated Rust that does
not compile.

### `init="boot"` / `init="deferred"`

Deferred construction is a convention, not a built-in keyword: a manifest opts
in by declaring the attribute itself.

```kdl
attr "init" kind="choice" default="boot" {
    choices "boot" "deferred"
    help "boot builds the device during Devices::new; deferred builds it in init()."
}
```

- `boot` (the default when the attribute is absent) constructs the device in
  `Devices::new`.
- `deferred` stores the bus handle or pins in a `Factory` and constructs the
  device when `app.rs` calls `init()`.
- A driver whose only runtime is `runtime "async"` must be deferred: there is
  no synchronous place to initialise it before boot.
- A deferred bus device needs `endpoint` in `codegen` so the factory knows what
  to store. A busless deferred device stores raw peripherals and needs none.

---

## 5. `codegen`: the Rust side

The `codegen` block is the **Rust code template** for your driver. It tells
`espforge` exactly what Rust struct type and constructor call to write into
`src/generated.rs` when a user puts your `<tag ... />` in their `.espf` file.

Everything else in the `.kdl` file describes the **ESPF side** (attributes, bus
kind, dependencies). `codegen` bridges that to the **Rust side**.

### 5.1 The easiest way to see it: input → generated output

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

`espforge` plugs those values into `codegen` and generates this in
`src/generated.rs`:

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
            // Filled from `constructor "..."`
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

### 5.2 `runtime "blocking" { ... }` and `runtime "async" { ... }`

A driver supports blocking mode, async mode, or both (one block for each).
Inside a `runtime` block there are up to six keys:

* **`type "..."`** *(required)*
  The Rust type of the field stored in `pub struct Devices`.
  *(You have to spell out the concrete generic parameters like `<CriticalSectionDevice<...>, Delay>` because Rust struct fields cannot use `impl Trait` or `_`.)*

* **`constructor "..."`** *(required)*
  The function call that builds the driver. It must be a plain function/method
  call (`Crate::Type::new(...)`) with a plain path — no turbofish. On a bus
  device its **first argument must always be `{bus_device}`** (or
  `{spi_device}`), or wrap that hole in the first argument. A busless driver
  has no bus to place and takes its own arguments in any order.

* **`endpoint "..."`** *(optional; required for a deferred or async bus device)*
  When a user sets `init="deferred"` (or uses an async driver, which is always
  deferred), `Devices::new` doesn't construct the driver at boot—it creates a
  `Factory` struct holding the bus handle until the user calls `.init()` in
  `app.rs`. `endpoint` is simply the **Rust type of `{bus_device}`** so
  `espforge` knows what type to put inside that `Factory` struct:
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

  An endpoint takes no `{placeholder}`: it is a type, not a template. A
  busless deferred device omits it, because its factory stores GPIO
  peripherals rather than a bus handle.

* **`awaits #false`** *(only inside `runtime "async"`, optional)*
  By default, an async factory calls `constructor(...).await`. If the crate's async `new()` is a regular synchronous `fn` (like `edrv_mhz19::MHZ19::new` in `mhz19b.kdl`), `awaits #false` tells `espforge` not to emit `.await`.

* **`feature "..."`** *(only inside `runtime "async"`, optional)*
  Overrides the Cargo feature on `espforge-drivers` when running in async mode (e.g., `ssd1306-async` instead of `ssd1306`).

* **`then { call "..." }`** *(optional, repeatable)*
  A method call the driver needs **after** it is constructed — `init()`, an initialisation sequence, a rotation:
  ```kdl
  runtime "async" {
      constructor "epd_waveshare::Epd::new({bus_device}, {dc}, {reset})"
      then { call "init({delay})" }
      then { call "set_rotation({rotation})" }
      then {
          call "flush()"
          awaits #false
      }
  }
  ```
  The generated code calls each step on the device it just built, in the order written, and a failure is that device's `Init` error, so a half-initialised value never reaches app code. A step names **no receiver** (write `init()`, not `self.init()`), and the only `{holes}` it may use are values the `constructor` already takes. `awaits` is a step of its own and defaults to the runtime's convention: on `runtime "async"` write `awaits #false` for a synchronous `fn`. A deferred device runs the same steps inside `init()`, not at boot. A step maps its error by default; write `fallible #false` for a setup call that returns `()` or `&mut Self`. Write `rebind #true` when the call consumes the device and returns the value to keep (`let mut dev = dev.init()?`): that is how a typestate `init(self) -> Result<Ready, E>` becomes the type the `Devices` field declares.

### 5.3 `map "<attr_name>" { ... }`

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

Then `{size}` can be substituted into both `type "Ssd1306Display<{size}>"` and `constructor "...({size}, {rotation})"`. A `type` map may fill a generic parameter; an `expr` map stands where a value is expected. `map` nodes may appear before or after the `runtime` blocks.

*(Clever trick used in `sht31.kdl` / `bme680.kdl`: if a driver needs `espforge_runtime::Delay::new()` as an argument, you can declare `attr "delay" kind="choice" default="runtime" { choices "runtime" }` and map `"runtime" { expr "espforge_runtime::Delay::new()" }` so `{delay}` injects the delay automatically without the user ever writing it in `.espf`.)*

### 5.4 `{placeholders}` vs fixed values

Inside `constructor "..."`, `espforge` replaces `{holes}` from a closed set checked at load time with `syn`:

* `{bus_device}` (or `{spi_device}`): the bus endpoint. It is the first argument of a bus device, or sits inside the first argument when that argument is a wrapper call (`Type::new(Wrapper::new({bus_device}), ..)`). A busless driver refuses it.
* `{address}`: the I²C 7-bit address as a number (`68` for `0x44`). It may stand as its own argument after the bus or inside the same wrapped first argument. It is refused when the bus declares no address, and on a non-I²C bus.
* `{cs_pin}`: the SPI chip-select line as a pin expression. SPI only.
* `{dc_pin}` or `{dc}`: a claimed `pin "..."` converted into an `Output`, `Input`, `Bidirectional` or `OpenDrainPin` per its `direction`. Both spellings mean the same line.
* `{any_mapped_attr}`: replaced by whatever `map "any_mapped_attr"` resolves to. Every non-pin attribute used as a hole must have a `map`.
* **Literal/fixed Rust expressions** (no braces!): anything else is passed verbatim to Rust—for example `&bosch_bme680::Configuration::default()` and `20` in `bme680.kdl`.

Only `{bus_device}`, `{spi_device}` and `{address}` may sit inside an
expression; every other hole must be an argument of its own. An unknown hole
is a load error naming the hole.

### 5.5 `fallible #false` (optional, defaults to `#true`)

This flag belongs to `codegen`, so it applies to both runtimes:

* Omit it if `constructor` returns `Result<Self, E>` (appends `.map_err(...)?`).
* Write `fallible #false` if `constructor` returns `Self` directly without a `Result` (like `sht4x.kdl` and `mhz19b.kdl`).

### 5.6 `feature "..."` (optional, bundled drivers only)

* Only used by drivers that live inside the `espforge-drivers` crate (like `ssd1306.kdl`).
* **Omit it** for manifest-only drivers that use a crates.io crate via `dependency "..."` directly (`sht31.kdl`, `bme680.kdl`, `ads111x.kdl`, `sht4x.kdl`, `mhz19b.kdl`).
* An async driver may use a different feature on its `runtime "async"` block, as `ssd1306.kdl` and `adxl345.kdl` do.

### 5.7 Why is `codegen` checked so strictly?

Because a KDL manifest is generating Rust code! `espforge-schema/src/driver.rs` parses `type`, `constructor`, `endpoint`, and every `map` entry through `syn` when loading the `.kdl` file—so if you make a typo in a Rust type or write an unknown `{placeholder}`, `espforge` catches it immediately when reading the manifest rather than spitting out broken Rust in `src/generated.rs`.

The same pass refuses a constructor that is not a call, a path with generic
arguments, a hole that appears twice, a `{bus_device}` that is not first, and
`{address}` on a bus that declared none.

---

## 6. Dependencies: the crates a manifest-only driver names

A driver without `feature` (one that is not built into `espforge-drivers`) names the crates it needs with `dependency`. Exactly one of `version`, `path` or `git` chooses the source, and `git` requires a pinned `rev`:

```kdl
dependency "embedded-sht3x" { version "0.2.0" }
```

Every dependency is written to the generated `Cargo.toml`, deduplicated by package. If two manifests name one package from different sources, generation stops with `DRIVER_DEPENDENCY_CONFLICT` and names both.

### Per-mode features

Some crates put their blocking and async APIs behind separate Cargo features, and some (like `bmi323-driver`) enable the async one by default. A single `features` list cannot express that — enabling both halves makes the crate fail to compile — so a dependency describes both modes:

| Key | Meaning |
|---|---|
| `features "..."` | Features the **blocking** mode needs. |
| `default_features #true` / `#false` | The blocking mode's `default-features`; defaults to `#true`. |
| `async_features "..."` | Features the **async** mode needs. |
| `async_default_features #true` / `#false` | The async mode's `default-features`; omit it to keep the blocking value. |
| `modes_are_exclusive #true` | The crate's two APIs are one Cargo feature each, so they must not both be on. |

```kdl
dependency "bmi323-driver" {
    version "0.2.0"
    features "blocking"            # the blocking half
    default_features #false        # ...which is the opt-in one here
    async_features "async"         # the async half
    async_default_features #true   # async is the crate's default
    modes_are_exclusive #true      # never compile both halves
}
```

Which set applies is the **board's** decision, not the manifest's: it resolves from the device's bus mode. An embassy project adds `async_features` to `features` and applies `async_default_features`. For an exclusive crate the async set *replaces* the blocking one, because Cargo features are additive and both halves would be compiled. Two devices of one exclusive crate in different modes are refused at lowering as `E0911` ("driver mode conflict"), naming both devices, the crate and both modes.

A `transport` block may amend the driver's dependency instead of repeating
its source (§7).

---

## 7. Multi-transport drivers: one tag, several buses

A chip that exposes the same register set over I2C or SPI becomes one manifest
with two or more `transport` blocks. A manifest uses **either** a top-level
`bus` + `codegen` **or** `transport` blocks, never both; fewer than two
transports is an error.

```kdl
// Illustrative: the two constructors and their types are elided.
driver "bme280" {
    format_version 2
    summary "Bosch BME280 pressure, temperature and humidity sensor"
    category "sensor"

    dependency "bme280" { version "0.5.0" }
    pin "sdo" direction="input" optional=#true

    attr "init" kind="choice" default="boot" {
        choices "boot" "deferred"
    }

    transport "i2c" {
        bus "i2c" { address allowed="0x76..=0x77" }
        codegen {
            runtime "blocking" {
                type "bme280::Bme280<...>"
                constructor "bme280::Bme280::new_i2c({bus_device}, {address}, {delay})"
                endpoint "..."
            }
        }
        dependency "bme280" { features "i2c" }
    }

    transport "spi" {
        bus "spi" {
            mode 0
            frequency "10000kHz"
        }
        codegen {
            runtime "blocking" {
                type "bme280::Bme280<...>"
                constructor "bme280::Bme280::new_spi({bus_device}, {delay})"
                endpoint "..."
            }
        }
        dependency "bme280" { features "spi" }
    }

    example "minimal" #"<bme280 id="air" bus="$i2c0" address="0x76" />"#
}
```

Rules:

- The shared nodes (`pin`, `attr`, `compose`, the driver-level `dependency`
  list) are declared once, outside the transports.
- Each `transport "<kind>"` names `i2c`, `spi`, `uart` or `none`, and contains
  its own `bus`, `codegen` and optional dependency amendments. `bus { ... }`
  inside a transport may omit the kind (it is the transport's) or repeat it;
  it must agree with the transport.
- The element accepts the **union** of the transports' bus attributes, all
  optional, because which ones are required depends on the bus the spec picks.
  Lowering selects the transport whose kind matches the resolved
  `bus="$..."` and enforces that transport's required `address` or `cs`.
- The `bus` attribute itself is optional when one of the transports is `none`:
  omitting `bus=` selects the busless variant. A missing `bus=` with no `none`
  transport is an error.
- A dependency inside a transport either amends a crate the driver already
  declares, naming features only, or repeats the source exactly. It cannot
  switch the crate to another version or git revision. Only the selected
  transport's features are emitted, so a project never builds both halves of a
  dual-interface crate.

---

## 8. `compose`: a driver plus its companion chips

Some parts are one physical module with several chips on the same bus — a
display with a touch controller, for instance. `compose` builds the companion
as a second device without the user writing it:

```kdl
driver "tft_touch" {
    format_version 2
    summary "ILI9341 display with an XPT2046 touch controller"
    category "display"

    bus "spi" {
        mode 0
        frequency "40000kHz"
    }

    pin "dc" direction="output" default_level="low"
    pin "touch_cs" direction="output" default_level="high"

    codegen {
        runtime "blocking" {
            type "..."
            constructor "..."
        }
    }

    compose "xpt2046" {
        attr "cs" from="touch_cs"
        attr "mode" value="0"
    }

    example "minimal" #"<tft_touch id="tft" bus="$spi2" cs="9" dc="4" touch_cs="10" />"#
}
```

Each `attr` names a companion attribute and where it comes from:

- `from="<name>"` reads one of the composite's own attributes or pins.
- `value="<literal>"` fixes it from the manifest.

The block is checked once the whole schema is loaded:

- the composed tag must exist and must be a driver;
- it must drive the **same bus kind** as the composite (a part on another bus
  is a separate element);
- the part must not itself compose, because expansion is one level deep;
- every attribute of the part must exist on it;
- every required attribute of the part except `id` and `bus` must be filled,
  unless the part declares a default;
- a `value=` literal must be a valid value of that attribute's kind and range.

At lowering, the composite and each part become separate devices on the
composite's bus, with the part's id derived as `<parent>_<tag>`. A duplicate id
or a pin conflict is reported normally.

---

## 9. Examples, see_also and what is validated when

Every manifest carries at least one `example "<title>" <fragment>`, with
distinct titles; a driver without one does not load. `espforge new` offers them
as starting points, and a contract test parses every fragment as an `.espf`
document.

`see_also "peripherals/i2c"` points at documentation; the target must be a
`section/leaf` pair.

Loading a manifest checks, before any project is generated:

- `format_version 2`, a summary, at least one example, and a `bus` (or two
  transports) with `codegen`;
- every node and property name, and every duplicate;
- every `type`, `constructor` and `endpoint` through `syn`, every `map` entry,
  and every `{placeholder}` against the closed set;
- the constructor's shape: a plain call, the bus first on a bus device, no
  repeated hole, `{address}` only where the bus has one, `{cs_pin}` only on
  SPI;
- the composed parts (§8) and the dependency sources;
- the constructor slots against the declared pins and attributes.

The regression corpus is `espforge-elements/tests/manifests.rs`; generated
code is checked by `espforge/tests/golden_projects.rs`,
`generated_projects_compile.rs`, `compile_fail.rs` and
`offline_generation.rs`. A new manifest needs an example and, for every new
rejection, a `compile_fail` case.

---

## 10. Not supported yet

These are deliberately absent; `plandriverfixes.md` tracks them and their
order.

- **CS-less SPI.** `cs_via` accepts only `"pin_or_ref"`, and `cs` is always a
  required attribute. `ws2812b`/`apa102` over SPI need a CS-less bus endpoint.
- **Struct-literal constructors and turbofish.** A `constructor` is a plain
  `Type::new(..)` path; `literal "Type { .. }"` and `Type::<Model>::new` are
  not implemented.
- **Other transports.** Bus kinds are `i2c`, `spi`, `uart` and `none`. There is
  no `<onewire>`, `<adc>`, or shared UART peripheral yet.
- **Manifest constraints and aliases.** No `requires_alloc` and no
  `alias "sht30"` tag aliases.
- **Deeper composition.** `compose` is one level; a composed driver may not
  compose another.
- **Interrupt pins.** `pin "..." irq=#true` has no generated edge-wait API.

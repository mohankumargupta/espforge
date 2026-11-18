# Journey: the v2 driver manifest

Phase 8 finishes `changes.md` §3: a driver is a `driver "<tag>"` file, it names
its crates and its Rust in a typed `codegen` block, and it can live in the
project, the bundled crate, or the fetched cache.

The old `element` + `device_recipe` form still loads for one release. It is
converted by `espforge driver migrate`, and the v1/v2 equivalence test
(`espforge-elements/tests/manifests.rs`) proves every bundled driver lowers the
same before and after the split.

---

## 1. A bundled driver

`espforge-drivers/manifests/ssd1306.kdl`:

```kdl
driver "ssd1306" {
    format_version 2
    summary "SSD1306 monochrome OLED display (0.96\" / 1.3\")"
    category "display"

    dependency "embedded-graphics" { version "0.8.1" }

    bus "i2c" {
        address allowed="0x08..=0x77"
    }

    codegen {
        feature "ssd1306"
        runtime "blocking" {
            type "espforge_drivers::Ssd1306Display<{size}>"
            constructor "espforge_drivers::Ssd1306Display::new({bus_device}, {address}, {size}, {rotation})"
            endpoint "espforge_drivers::I2cEndpoint"
        }
        runtime "async" {
            feature "ssd1306-async"
            type "espforge_drivers::AsyncSsd1306Display<{size}>"
            constructor "espforge_drivers::AsyncSsd1306Display::new({bus_device}, {address}, {size}, {rotation})"
            endpoint "espforge_drivers::AsyncI2cEndpoint"
        }
        map "size" {
            "128x64" { type "espforge_drivers::Oled128x64" }
            "128x32" { type "espforge_drivers::Oled128x32" }
        }
        map "rotation" {
            "0" { expr "espforge_drivers::DisplayRotation::Rotate0" }
            "90" { expr "espforge_drivers::DisplayRotation::Rotate90" }
            "180" { expr "espforge_drivers::DisplayRotation::Rotate180" }
            "270" { expr "espforge_drivers::DisplayRotation::Rotate270" }
        }
    }

    attr "size" kind="choice" default="128x64" { choices "128x64" "128x32" }
    attr "rotation" kind="choice" default="0" { choices "0" "90" "180" "270" }
    example "minimal" #"<ssd1306 id="oled" bus="$i2c0" address="0x3C" />"#
    see_also "peripherals/i2c"
}
```

`id` and `bus` are synthesised. `bus "i2c"` synthesises `address`; `bus "spi"`
synthesises `cs` and, when it declares them, `mode`/`frequency`. A `pin`
declares the element attribute of the same name.

## 2. Typed codegen

No manifest contains raw Rust. `codegen.type` is parsed as a `syn::Type` and
`codegen.constructor` as a call expression; map values are parsed as a type or
an expression. Before parsing, each `{hole}` is replaced by a sentinel
identifier, so a manifest author cannot escape the typed check.

The closed placeholder set is:

- every `attr` name with a `map`;
- every `pin` name (`{dc}` or `{dc_pin}`);
- `{bus_device}` or `{spi_device}` first;
- `{address}` immediately after the bus for an I2C driver that needs one;
- `{cs_pin}` where an SPI driver wants the chip select named explicitly (the
  endpoint normally owns it).

The emitter writes a pin from a fixed template. A manifest chooses
`default_level="low"` or `"high"` and `optional=#true`; it cannot choose the
Rust. The v2 parser refuses an unknown placeholder, a malformed type, a
placeholder inside a string literal, a duplicate argument, a pin that is not a
constructor argument, and blocking/async constructors with different argument
orders.

A constructor argument may also be an expression the manifest already knows.
Everything after the bus is parsed positionally: a `{hole}` is a slot the
element fills, and any other expression is passed through verbatim —
`ads111x::Ads111xConfig::default()`, `&bme680::Configuration::default()`, `20`.
A fixed argument keeps the position the manifest gave it, for boot, deferred and
async emission alike, and may not contain a `{placeholder}`. `{address}` still
has to sit directly after the bus.

`endpoint` is a Rust *type*, not a path: it names the type the generated bus
helper returns, so a manifest-only driver can write the real thing
(`espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static,
esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>`). It takes no
placeholder; only `type` and `constructor` are templates.

A crate whose `new()` returns the device rather than a `Result` says so with
`codegen { fallible #false }`. Emission then assigns the device directly and a
deferred factory wraps it in `Ok(..)`; `.map_err(..)` is written only for a
fallible constructor. The flag needs an external crate — a bundled driver
returns `DeviceErrorKind`, so pairing it with `feature` is refused.

## 3. Dependencies

```kdl
dependency "espforge-driver-fx-accel" {
    path "../fx-accel-driver"
}
dependency "embedded-graphics" {
    version "0.8.1"
    default_features #false
    features "..." 
}
```

Exactly one of `version`, `path` or `git` names the source; `git` requires a
pinned `rev`. Every dependency is emitted to `Cargo.toml`, deduplicated by
package. Two manifests naming one package with different sources are refused
with `DRIVER_DEPENDENCY_CONFLICT` and both paths. A bundled driver keeps its
`codegen.feature`; an external driver names at least one `dependency`.

`default_features #false` is honoured verbatim. A crate whose blocking and async
APIs are different `Cargo` features — `bmi323-driver` puts async on by default
and `blocking` behind an opt-in — describes both halves:

```kdl
dependency "bmi323-driver" {
    version "0.2.0"
    features "blocking"            # the blocking half
    default_features #false        # ...which is the opt-in one here
    async_features "async"         # the async half
    async_default_features #true   # async is the crate's default
    modes_are_exclusive #true      # the two must not both be on
}
```

`features`/`default_features` describe the blocking mode and
`async_features`/`async_default_features` the async one. Which applies is the
*board's* decision, resolved from the device's bus, not the manifest's. For an
exclusive crate the async set replaces the blocking one, because `Cargo`
features are additive; for any other crate the async features are added. Two
devices of one exclusive crate in different modes are refused at lowering as
`E0911` ("driver mode conflict"), naming both devices, the crate and both
modes.

## 4. Fetching and the cache

`espforge driver add` accepts:

- a path: validated and copied into `drivers/`, or recorded in place with
  `--local`;
- `https://…tar.gz`: downloaded with a 32 MiB cap, optionally pinned with
  `--sha256`, and extracted with a 4096-entry / 64 MiB cap. Absolute paths,
  `..`, symlinks, hardlinks and device nodes are refused before extraction;
- `git+https://…#<rev>`: the `git` binary with no prompt, no system config, no
  hooks and no submodules; the full commit SHA is recorded;
- `<tag>@<version> --registry https://…/index.json`: an exact version from the
  static index (`{"schema":1,"drivers":{…}}`).

A fetched package is installed at
`$XDG_CACHE_HOME/espforge/drivers/<tag>/<version>/`:

```
driver.kdl
crate/…
source.json
```

`source.json` records the kind, original source, requested revision and commit.
`.espforge/drivers.lock` version 2 records:

```json
{
  "version": 2,
  "drivers": {
    "fx_accel": {
      "kind": "git",
      "source": "https://example.invalid/fx-accel",
      "requested": "v0.1.0",
      "commit": "…",
      "manifest_sha256": "…",
      "payload_sha256": "…",
      "dependencies": [{ "package": "espforge-driver-fx-accel", "path": "crate" }],
      "cache_path": "…/driver.kdl"
    }
  }
}
```

`espforge driver verify` re-hashes the manifest and payload against the lock;
`--refresh` re-fetches the recorded source. `remove --prune-cache` deletes an
entry no lock refers to.

`check`, `gen` and `build` never fetch: they read `drivers/`, `--driver-dir` and
the cache. Only `add`, `update` and `verify --refresh` reach the network.

## 5. Migration

`espforge driver migrate <file>` rewrites a v1 `element` manifest as a v2
`driver` file, prints it by default, and writes it with `--write`. Pins become
`pin` declarations, `needs_address` becomes `bus "i2c"`, `crate`/`expose`
become `dependency` blocks, and the v1 `type`/`new`/`arg` data becomes
`codegen`. A pin template that v2 cannot express (an input pin or arbitrary
Rust) is refused with the driver and pin named.

The frozen v1 file at `testing/fixtures/schema-v1/drivers.schema.kdl` and the
equivalence test are deleted one release after v2 ships, together with the v1
branch in the lowerer.

## 6. What stays out of KDL

Three shapes are deliberately left as Rust glue. Writing them as manifest
templates would turn `codegen` into a macro language, and each needs code that
composes two or three upstream objects rather than naming one constructor.

1. **Struct-literal constructors.** `ws2812-rs` builds its driver as
   `WS2812SPI { led: spi }`. A struct literal has no `new` path, so the
   constructor rule — a call on a plain path — refuses it. A dedicated driver
   crate with a one-line constructor is the honest answer, not a template that
   can spell a literal.
2. **A device with no chip select on SPI.** `<spi>`'s `cs` is optional for the
   bus, and `cs_via "pin_or_ref"` covers the parts that use one. A part that
   drives the bus with no select at all is a bus-level decision — it belongs
   with `provides` (expanders, muxes, multi-peer buses), not with a driver.
3. **Builders, typestates and composed panels** (`hd44780-embedded-hal`,
   `epd-waveshare-async`, `epdsi`, `icm20948-async`, `lora-phy`), and crates
   whose bus type is not exported: `stts22h-rs` and `bmi323-driver` construct a
   typestate (`Stts22h<I2cBus<P>, OnState>`) whose bus wrapper and bound are not
   public, so neither `type` nor `endpoint` can name the field. Each of these
   needs a small function that builds the device from the bus espforge already
   hands out — one function per crate, not one wrapper struct per crate, planned
   in a follow-up `driver-glue.md`.

The manifest's job ends where a value must be *computed*: it names a
constructor, the constants that constructor takes, and the element attributes
that fill its slots — it does not build an expression that assembles several
values into a device.

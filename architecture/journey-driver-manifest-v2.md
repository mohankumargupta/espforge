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

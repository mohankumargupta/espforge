In `espforge`, "variants that use the same driver" shows up in two ways across real crates:

1. **Identical-protocol part numbers** handled by one crate (e.g. `sht31.kdl` covers SHT30/SHT31/SHT35 via `embedded-sht3x`, `sht4x.kdl` covers SHT40/SHT41/SHT45, `scd4x.kdl` covers SCD40/SCD41).
2. **Chip families in one crate** where variants share a dependency but need a different generic marker, constructor name, I²C address rule, or panel dimensions (e.g. `ssd1306` `128x64` vs `128x32`, `ads1x1x` `new_ads1115` vs `new_ads1015`, `mipidsi` for ILI9341/ST7789/GC9A01).

Because `codegen` currently allows only one `constructor` path per runtime and `map` can only fill `{placeholders}` inside `type` and constructor arguments, here are **four options** to handle variants:

---

### Option 1: Single "family" tag + `attr` + `map` *(works today, zero code changes)*

Keep one manifest under a family name (`sht3x`, `scd4x`, `ssd1306`) and distinguish variants via an `attr` + `codegen { map ... }`—exactly how `espforge-drivers/manifests/ssd1306.kdl` handles `size="128x64"` vs `"128x32"`:

```kdl
driver "ssd1306" {
    ...
    codegen {
        runtime "blocking" {
            type "espforge_drivers::Ssd1306Display<{size}>"
            constructor "espforge_drivers::Ssd1306Display::new({bus_device}, {address}, {size}, {rotation})"
        }
        map "size" {
            "128x64" { type "espforge_drivers::Oled128x64" }
            "128x32" { type "espforge_drivers::Oled128x32" }
        }
    }
    attr "size" kind="choice" default="128x64" { choices "128x64" "128x32" }
}
```

* **Pros:** Zero changes to `espforge`; works immediately for identical chips (`scd40`/`scd41` under `scd4x`) or variants that only swap a type/config argument (`size`, `resolution`).
* **Cons:**
  * `map` cannot swap the **constructor function path** itself (e.g. `Ads1x1x::new_ads1115` vs `Ads1x1x::new_ads1015`) or vary `bus` address constraints per variant.
  * Discoverability suffers: a user with an **SHT30** or **ADS1115** who writes `<sht30 ... />` or `<ads1115 ... />` gets `DEVICE_UNSUPPORTED` unless they know the manifest calls it `<sht31>` or `<ads111x>`.

---

### Option 2: Separate `.kdl` manifests per variant sharing the same `dependency` *(works today, zero code changes)*

Write one small manifest per concrete part number (`sht30.kdl`, `sht31.kdl`, `sht35.kdl`, or `ads1115.kdl`, `ads1015.kdl`), all naming the exact same `dependency "<crate>" { version "..." }`.

This already works cleanly in `espforge` because `Board::driver_dependencies()` (`espforge-elements/src/model.rs`) deduplicates dependencies by crate name (`same_source` + `absorb`). If a user puts both `<ads1115>` and `<ads1015>` on one board, `Cargo.toml` emits the crate dependency **once**.

* **Pros:**
  * Zero compiler/schema changes.
  * Every physical part gets its exact name (`<sht30>`, `<ads1115>`, `<st7789>`).
  * Each variant can have a **completely different `constructor` path**, different `address` rules (`fixed` vs `allowed`), and different defaults.
* **Cons:**
  * Copy-paste duplication across `.kdl` files (`summary`, `dependency`, `attr`s, `codegen` boilerplate). Upgrading a crate version means editing every variant manifest that points to it.
  * Each bundled manifest also adds an entry to `espforge-drivers/src/lib.rs` and updates the bundled count in workspace tests.

---

### Option 3: Declarative `alias` (or `aliases`) in v2 manifests *(small schema + `build.rs` addition)*

Extend `espforge-schema/src/driver.rs` so one manifest can register multiple ESPF tag names for the same driver definition:

```kdl
driver "sht3x" {
    format_version 2
    alias "sht30" "sht31" "sht35"
    ...
}
```

* **How it works in `espforge`:**
  * `parse_driver` registers the primary tag plus each alias in `schema.elements` (checking uniqueness across all tags so duplicates are still refused with `duplicate tag`).
  * `espforge-elements/build.rs` emits `pub use sht3x as sht30;` (or generates matching `Attrs` modules) and adds the aliases to `validate_function`.
  * `espforge-elements/src/devices/recipe.rs` lowers `<sht30>` identically to `<sht3x>`.
* **Pros:**
  * Zero KDL duplication for protocol-identical variants (`sht30`/`sht31`/`sht35`, `sht40`/`sht41`/`sht45`, `scd40`/`scd41`, `mhz19`/`mhz19b`).
  * Users can write `<sht30 ... />` naturally without guessing whether the manifest chose `sht30`, `sht31`, or `sht3x`.
* **Cons:**
  * Only solves **identical** variants (same `type`, same `constructor`, same `bus` constraints). It won't help when variants differ in constructor name (`new_ads1115` vs `new_ads1015`) or address range.
  * Error messages like `known: <led>, <button>, ...` in `lower.rs` get longer unless aliases are filtered or grouped.

---

### Option 4: Parameterized `variant` presets in KDL (or a thin family factory in `espforge-drivers`)

For driver crates where variants share 90% of the manifest (`dependency`, `bus`, `attr`s, `map`s) **except** the `constructor` function, `type`, or a fixed config/address:

* **Variant A — `variant` blocks in KDL v2:** Allow a `driver` to declare named presets (`variant "ads1115" { ... }`, `variant "ads1015" { ... }`), selected either by an `attr "model"` or by tag, where each `variant` can override `codegen.constructor`, `codegen.type`, or `bus.address` while inheriting everything else from the parent `driver`.
* **Variant B — Thin Rust family glue in `espforge-drivers`:** (The `driver-glue.md` pattern already noted in `architecture/journey-driver-manifest-v2.md` §6 for `mipidsi` / builders / typestates.) Write one small factory function in Rust that takes a `Model` enum (from a KDL `map "model"`) and dispatches `match model { ... }` to call `new_ads1115` vs `new_ads1015` or `Builder::ili9341` vs `Builder::st7789`.

* **Pros:**
  * Solves the "different constructor paths / builder patterns" case (like `ads1x1x` and `mipidsi`) without duplicating 5 nearly identical `.kdl` files.
  * Variant B also handles crates that use builders/typestates that KDL `codegen` deliberately stays out of.
* **Cons:**
  * Variant A adds significant complexity and inheritance rules to `espforge-schema/src/driver.rs` (`parse_driver`).
  * Variant B requires writing a small Rust wrapper in `espforge-drivers` instead of staying 100% manifest-only.

---

### Recommendation

Use a **two-tier rule** depending on what kind of variant it is:

1. **For identical chips or pure type/argument variants (`sht30/31/35`, `scd40/41`, `ssd1306` sizes):**
   * Use **Option 1** today (`sht3x` / `scd4x` / `ssd1306` with `attr` + `map`), and if you find users tripping over exact part names like `<sht30>`, add **Option 3 (`alias`)** as a small, clean schema upgrade.
2. **For chip families with different constructors or address rules (`ads1015` vs `ads1115`, `bme280` vs `bmp280`):**
   * Use **Option 2** (separate `.kdl` manifests sharing the same `dependency` crate) first—it works out of the box today, `Board::driver_dependencies()` already deduplicates the crate in `Cargo.toml`, and it avoids turning KDL into an inheritance language. Only reach for **Option 4B** (`driver-glue` Rust factory) when a crate like `mipidsi` uses builders/typestates that KDL can't express directly.

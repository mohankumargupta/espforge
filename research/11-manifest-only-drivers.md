

# `manifest-only.md`: Roadmap to Manifest-Only Drivers

**Baseline:** `espforge` `0.9.3` (which already includes `format_version 2` manifests, `fallible #false`, fixed constructor expressions, generic `endpoint`s, `then { call ".." }`, `bus "none"`, `PinOrRef`/`open_drain` pins, per-mode dependency features, and `espforge driver add|update|verify` cache/registry support) and the 29-driver audit in `manifests.md`.

**Goal:** move from **3 of 29** manifest-only drivers (`hd44780`, `max7219`, `nxp74hc595`) to **29 of 29** using `dependency { version ... }` or `dependency { git ... rev ... }` with no per-project `../<tag>-driver` source crates.

---

## 1. Current Inventory (from `manifests.md`)

| Bucket | Count | Manifests | What moves them |
|---|---|---|---|
| **Already manifest-only** | 3 | `hd44780`, `max7219`, `nxp74hc595` | Done |
| **Convertible today** | 7 | `aip31068`, `bh1750`, `sgp30`, `mcp9808`, `bme280_i2c`, `bme280_spi`, **`ds3231`** | Manifest + `app.rs` edits only (0 `espforge` changes) |
| **Blocked by small `espforge` schema/codegen gaps** | 8 | `aht20`, `mfrc522`, `ccs811`, `pcf857x`, `pca9685`, `hdc302x`, `sen6x`, `ina219` | 3 targeted additions to `espforge-schema` and `espforge-elements` |
| **Blocked by `embedded-hal` 0.2-only crates** | 7 | `ads1x15`, `am2320`, `ht16k33`, `mcp23xxx`, `mpu9250`, `tlv493d`, `tmp102` | Upstream `eh` 1.0 crate (or publish the local crate) |
| **No published crate exists** | 4 | `tmp123`, `as7262`, `bmxx80`, `tca95xx` | Publish the local crate (or package via registry) |

> **Note on `ds3231`:** `ds323x` 0.7.0 is `embedded-hal` 1.0 and exposes `Ds323x::new_ds3231(dev)` (infallible, hardcoded `0x68`, no address argument). With `address fixed="0x68"` and `fallible #false`, it fits in the "convertible today" group alongside `mcp9808` and `sht4x`.
>
> **Note on `pca9685` and `hdc302x`:** `pwm-pca9685` 1.0.0 takes `pwm_pca9685::Address` (a struct/tuple rather than `u8`), and `hdc302x` 0.5.0 uses `hdc302x::I2cAddr`. Both sit in the `pcf857x` address-mapping bucket unless they implement `From<u8>`.

---

## 2. Two Separate Tracks

Moving the remaining 26 manifests splits into **two independent tracks**:

1. **Compiler & Manifest Expressiveness (Phases 1–2):** converting the 7 ready drivers immediately and closing the 3 schema/emitter gaps that block 8 more `embedded-hal` 1.0 crates.
2. **Crate Ecosystem Supply (Phase 3):** handling the 11 drivers where no suitable `embedded-hal` 1.0 crate exists on crates.io (`0.2`-only or missing). No KDL feature can invent a missing crate, so "manifest-only" here means publishing or git/registry-hosting clean `embedded-hal` 1.0 crates.

---

## Phase 1: Convert the 7 Drivers That Work Today (No `espforge` Changes)

All 7 use `embedded-hal` 1.0 crates whose constructors fit `espforge` `0.9.3` as-is:

| Manifest | Target Crate | Constructor / `then` Shape | Required Element / `app.rs` Adjustment |
|---|---|---|---|
| `aip31068.kdl` | `i2c-character-display` `0.6.0` | `CharacterDisplayAIP31068::new({bus_device}, LcdDisplayType::Lcd16x2, espforge_runtime::Delay::new())` | Drop `{address}` from constructor (chip is fixed at `0x3E`; `address fixed="0x3E"` is legal without `{address}`) |
| `bh1750.kdl` | `bh1750` `0.1.0` | `BH1750::new({bus_device}, {delay}, false)` | Add `attr "delay"` + `map "delay"` |
| `sgp30.kdl` | `sgp30` `1.0.0` | `Sgp30::new({bus_device}, {address}, {delay})` | Call `init()` in `app.rs` |
| `mcp9808.kdl` | `mcp9808` `0.4.0` | `MCP9808::new({bus_device})` | Narrow `address` to `fixed="0x18"` (crate hardcodes `0x18` and takes no address arg) |
| `ds3231.kdl` | `ds323x` `0.7.0` | `ds323x::Ds323x::new_ds3231({bus_device})` | Add `fallible #false`; narrow `address` to `fixed="0x68"` (if `address` was configurable) |
| `bme280_i2c.kdl` | `bme280` `0.5.1` | `bme280::i2c::BME280::new({bus_device}, {address})` + `then { call "init(&mut espforge_runtime::Delay::new())" }` | Add `fallible #false`; `then` handles `init` (see check below); adjust expected fixture readings for 2x T / 16x P / IIR 16 |
| `bme280_spi.kdl` | `bme280` `0.5.1` | `bme280::spi::BME280::new({bus_device})` + `then { call "init(&mut espforge_runtime::Delay::new())" }` | Same as `bme280_i2c.kdl` |

### Why `bme280`'s `then` step works in `0.9.3`
In `espforge-schema/src/driver.rs` (`parse_step`, `parse_steps`), `call "init(&mut espforge_runtime::Delay::new())"` has zero `{...}` placeholders, so `parse_steps` validates it as a `syn::ExprMethodCall` on `__espforge_receiver` with no unknown-arg errors. In `espforge-elements/src/model.rs` (`step_statements`), it emits:

```rust
let mut dev_device = bme280::i2c::BME280::new(...);
dev_device
    .init(&mut espforge_runtime::Delay::new())
    .map_err(|_| espforge_runtime::DeviceError {
        device: "dev",
        kind: espforge_runtime::DeviceErrorKind::Init,
    })?;
```

Because `bme280::BME280::init` returns `Result<(), Error<E>>`, the fallible `.map_err(...)?` matches cleanly. *(Note: `bme280_i2c` and `bme280_spi` can also be unified later into a single `<bme280>` manifest using `transport "i2c"` and `transport "spi"`, which `espforge-schema/src/driver.rs` already supports.)*

### Example: `ds3231.kdl` (target shape)
```kdl
driver "ds3231" {
    format_version 2
    summary "DS3231 real-time clock"
    category "clock"

    dependency "ds323x" { version "0.7.0" }

    bus "i2c" {
        address fixed="0x68"
    }

    codegen {
        fallible #false
        runtime "blocking" {
            type "ds323x::Ds323x<ds323x::interface::I2cInterface<espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>>, ds323x::ic::DS3231>"
            constructor "ds323x::Ds323x::new_ds3231({bus_device})"
            endpoint "espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>"
        }
    }

    attr "init" kind="choice" default="boot" {
        choices "boot" "deferred"
    }
    example "minimal" #"<ds3231 id="rtc" bus="$i2c0" address="0x68" />"#
}
```

### Per-driver conversion checklist
1. Replace `dependency "<tag>-driver" { path ... }` with `dependency "<crates-io-pkg>" { version "..." }`.
2. Update `codegen` (`type`, `constructor`, `endpoint`, `fallible #false`, `then`, `map "delay"` as needed).
3. Update `description` to remove `"The Rust half is the <tag>-driver crate beside this manifest"`.
4. Update the fixture's `src/app.rs` to call the published crate's measurement/control methods.
5. Run `espforge check`, `espforge driver update <tag>`, `espforge build`, and the emulator test (`just <tag>-sensor`).
6. Delete the unused `../<tag>-driver` directory once the emulator test passes.

---

## Phase 2: Close the 3 `espforge` Codegen Gaps (Unlocks 8 More Drivers)

These 8 crates are already `embedded-hal` 1.0, well-maintained, and blocked only by three small, additive extensions to `format_version 2`. None break existing manifests.

### Gap 2A: Non-bus-first constructor order (`sen6x`)
* **Problem:** `Sen6x::new(&mut timer, i2c)` puts the bus **second**, not first. Today `parse_constructor` in `espforge-schema/src/driver.rs` rejects `{bus_device}` if `!args.is_empty()`, and `BusDevice::emit` in `espforge-elements/src/model.rs` unconditionally prepends `#handle` in `constructor_args`.
* **Change:**
  1. Add `CtorArg::Bus` to `espforge-schema/src/lib.rs` (just like `CtorArg::Address` was added in `driverfixes.md` Milestone 3.1).
  2. In `espforge-schema/src/driver.rs` (`parse_constructor`), record `CtorArg::Bus` at the exact index `{bus_device}` or `{spi_device}` appears instead of requiring it at index `0` and removing it. Keep requiring that a bus device's constructor uses `{bus_device}`/`{spi_device}` (or wraps it in `CtorArg::Fixed`), and require `{address}` to stand on its own or in a wrapped arg rather than hardcoding `index == 0` relative to the tail.
  3. In `espforge-elements/src/devices/recipe.rs` (`slots`) and `espforge-elements/src/model.rs` (`constructor_args` + deferred factory call), emit `#handle` / `self.endpoint` at `CtorArg::Bus`'s position.
* **Unlocks:** `sen6x.kdl` (directly uses `sen6x` `0.1.4`).

### Gap 2B: Address mapping & `{address}` expressions (`pcf857x`, `pca9685`, `hdc302x`, `ina219`)
* **Problem:** Many `embedded-hal` crates (`pcf857x`, `pwm-pca9685`, `hdc302x`, `ina219`) take an address **enum or newtype** (`SlaveAddr`, `Address`, `I2cAddr`) instead of a raw `u8`. Today:
  * `build_recipe` (`espforge-schema/src/driver.rs`) only permits `map` on `Choice` or `Bool` attributes (`address` is `AttrKind::Hex`).
  * `wrapped_argument` rejects `{address}` in a standalone argument like `SlaveAddr::from({address})` unless `{bus_device}` is in the same argument (`if buses == 0`).
* **Change (both are additive and complement each other):**
  1. **Allow `{address}` inside a non-bus argument (`wrapped_argument`):** relax `wrapped_argument` in `espforge-schema/src/driver.rs` so `{address}` may sit inside an expression on its own argument (e.g., `hdc302x::I2cAddr::try_from({address}).unwrap()` or `pwm_pca9685::Address::from(...)`). In `espforge-elements/src/devices/recipe.rs::slots()`, `{address}` inside `CtorArg::Fixed` is already replaced at lowering time (`text.replace("{address}", ...)` — switch this to `RustExpr::splice` via `syntax.rs` so string literals are safe).
  2. **Allow `map "address"` for discrete hex enums:** in `espforge-schema/src/driver.rs` (`build_recipe`), allow `map "address"` when `bus "i2c"` has `address allowed="low..=high"` (checking that every integer in `low..=high` is mapped, e.g. `"0x20"`..`"0x27"` for `pcf857x::SlaveAddr::Alternative(...)`). In `devices/recipe.rs::slots()`, if a `map "address"` exists, resolve `format!("0x{got:02X}")` through it instead of emitting `RustExpr::number(got)`.
* **Unlocks:** `pcf857x.kdl`, `pca9685.kdl`, `hdc302x.kdl` (and the address half of `ina219.kdl`).

### Gap 2C: `then` step rebinding & `fallible #false` (`aht20`, `mfrc522`, `ccs811`, `ina219`)
* **Problem:** Typestate drivers construct an `Uninitialized` handle and transform it via `init(self)` into the `Initialized` type stored on `Devices` (`aht20-driver`, `mfrc522`, `embedded-ccs811`, `ina219`). Today:
  * `step_statements` (`espforge-elements/src/model.rs`) only emits `#call ...?;` without rebinding the local, so a consuming method (`init(self) -> Result<Ready, E>`) discards the returned device or fails to compile.
  * Every `then` step unconditionally appends `.map_err(...)?;`, breaking setup methods that return `()` or `&mut Self`.
* **Change:**
  1. Add `rebind #true` (default `#false`) and `fallible #false` (default `#true`) to `StepDecl` / `Step` in `espforge-schema/src/driver.rs` (`parse_step`) and `espforge-schema/src/lib.rs`.
  2. In `espforge-elements/src/model.rs` (`step_statements`), when `step.rebind` is true, emit:
     ```rust
     let mut #local = #call #awaited #step_check;
     ```
     Because `let mut #local = #constructed;` has no explicit type annotation and `Devices::new` returns `Ok(Self { #name: #local })`, Rust infers the intermediate `Uninitialized` type automatically while `type "..."` on the `Devices` struct names the final post-init type!
  3. Only append `.map_err(|_| DeviceError { ... })?` when `step.fallible` is true.
  4. *(Optional for `ccs811`:)* Check `embedded-ccs811`'s `nwake` pin signature (`Pin` vs `OutputPin`); if it accepts `espforge_runtime::OutputPin` (or `Option<OutputPin>`), `ccs811` works directly once `rebind #true` lands.
* **Unlocks:** `aht20.kdl`, `mfrc522.kdl`, `ccs811.kdl`, `ina219.kdl`.

### Example: `aht20.kdl` after Phase 2C
```kdl
codegen {
    fallible #false
    runtime "blocking" {
        type "aht20_driver::AHT20Initialized<espforge_runtime::embedded_hal_bus::i2c::CriticalSectionDevice<'static, esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>, espforge_runtime::Delay>"
        constructor "aht20_driver::AHT20::new({bus_device}, aht20_driver::SENSOR_ADDRESS)"
        then {
            call "init(&mut espforge_runtime::Delay::new())"
            rebind #true
        }
    }
}
```

---

## Phase 3: Close the Ecosystem Gaps (The Remaining 11 Drivers)

No manifest syntax change can turn an `embedded-hal` 0.2 crate into a 1.0 crate, or create a crate that does not exist. For these 11, there are three pragmatic strategies—choose per driver rather than forcing one rule on all 11.

### Strategy A: Swap to an existing `embedded-hal` 1.0 crate (`ads1x15`)
* **`ads1x15.kdl`:** `espforge` already ships `espforge-drivers/manifests/ads111x.kdl` backed by `embedded-ads111x` `0.1.3` (`embedded-hal` 1.0). If 16-bit ADS111x coverage is sufficient (or `embedded-ads111x` covers your variants), migrate `ads1x15.kdl` to `embedded-ads111x` (or an `ads1015`/`ads1115` manifest pair) instead of maintaining a local crate.
* For the other 6 (`am2320`, `ht16k33`, `mcp23xxx`, `mpu9250`, `tlv493d`, `tmp102`), re-check crates.io for newer `eh` 1.0 forks/crates (e.g. `-eh1`, `-async`, or `eldruin` 1.0 updates) before publishing your own.

### Strategy B: Publish your local `*-driver` crates (Crates.io or Git + Static Registry)
For the 4 parts with no crate (`tmp123`, `as7262`, `bmxx80`, `tca95xx`) and any of the 7 `0.2`-only parts without an upstream 1.0 release, your local `*-driver` crates **already implement `embedded-hal` 1.0 in 100–577 lines**.

To make their manifests genuine **manifest-only** files (portable outside this workspace, no `../<tag>-driver` path):

1. **Standardize crate metadata & surface:**
   * Ensure each crate is `#![no_std]`, depends only on `embedded-hal = "1"` (and optionally `embedded-hal-async` / `embedded-hal-bus`), and has **zero `esp-hal` or `espforge-runtime` dependency**.
   * Ensure `new(...)` takes `(bus, ...)` first so any user or manifest can call it cleanly.
2. **Choose your distribution channel (all three work in `espforge` `0.9.3` today):**
   * **crates.io (best for public sharing):** `cargo publish` each crate, then change the manifest from `path "../<tag>-driver"` to `version "0.1.0"`.
   * **Git repository + pinned commit (zero crates.io clutter):** host the crates in one workspace repo (e.g. `espforge-driver-crates`) and reference them in each manifest with:
     ```kdl
     dependency "tmp123-driver" {
         git "https://github.com/<org>/espforge-driver-crates"
         rev "<40-char-sha>"
     }
     ```
   * **Static registry index (`espforge driver add <tag>@0.1.0 --registry <url>`):** package each `<tag>/driver.kdl` + `crate/` as a `.tar.gz` and serve a static `index.json` (`espforge-core/src/fetch.rs` already implements SHA-256 verification, safe tar extraction, and `$XDG_CACHE_HOME/espforge/drivers/` caching).

> **Why avoid an `embedded-hal` 0.2 compatibility shim in `espforge-runtime`:** wrapping `CriticalSectionDevice` in `embedded-hal-compat::Forward` changes the `type "..."` generics to `Forward<CriticalSectionDevice<...>>`, fails cleanly only for traits without `nb` (`nb::Result`), and requires adding `embedded-hal = "0.2"` + `embedded-hal-compat` to every generated project. Publishing clean `eh` 1.0 crates (Strategy B) is less brittle and leaves better Rust types in `generated.rs`.

---

## Phase 4: Verification & Cleanup Workflow

Apply this gate after **each** manifest conversion (Phase 1), **each** `espforge` change (Phase 2), and **each** published crate (Phase 3):

1. **Workspace & schema checks (when modifying `espforge`):**
   ```sh
   cargo test --workspace
   cargo test -p espforge-elements --test manifests
   cargo test -p espforge --test golden_projects -- --ignored
   cargo test -p espforge --test generated_projects_compile -- --ignored
   cargo test -p espforge --test compile_fail -- --ignored
   ```
   *(Ensure zero unapproved golden churn; every new `driver.rs` rule needs a schema test and a compile/compile-fail case.)*

2. **Per-manifest conversion gate (in the driver workspace):**
   * `espforge check` on the fixture `.espf` passes.
   * `espforge driver update <tag>` refreshes `.espforge/drivers.lock` (`manifest_sha256` and `dependencies`).
   * `espforge build` compiles for `riscv32imc-unknown-none-elf`.
   * Emulator run (`just <tag>-sensor`) passes against the hardware model.
   * Delete the superseded `../<tag>-driver` crate directory (or move any pure protocol unit tests you still care about if you are publishing that crate under Strategy B).

---

## Summary Milestones

| Milestone | Scope | Manifest-Only Total |
|---|---|---|
| **Start (`0.9.3`)** | `hd44780`, `max7219`, `nxp74hc595` | **3 / 29** |
| **After Phase 1** | + `aip31068`, `bh1750`, `sgp30`, `mcp9808`, `ds3231`, `bme280_i2c`, `bme280_spi` | **10 / 29** |
| **After Phase 2** | + `sen6x`, `pcf857x`, `pca9685`, `hdc302x`, `aht20`, `mfrc522`, `ccs811`, `ina219` | **18 / 29** |
| **After Phase 3** | + `ads1x15` (via `embedded-ads111x`) + 10 published/git-pinned `eh` 1.0 crates | **29 / 29** |

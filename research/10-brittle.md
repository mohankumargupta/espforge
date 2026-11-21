Here is an analysis of the **current architectural brittleness** in the `espforge` (`0.9.3`) codebase. (Older issues already documented and resolved—such as singleton `StaticCell`s in `I2cBus`/`SpiBus`, `Option::take()` bus stealing, non-transactional `ResourceGraph` claims, and `Cargo.toml` drift—are excluded; every item below exists in the current code.)

---

### 1. Silent Identifier & Peripheral Collisions That Escape to `rustc`

* **No duplicate `id` checking across lowered elements:** `LowerCx::checked_ident` (`espforge-elements/src/element.rs`) only checks `RESERVED_IDENTS`, leading `__`, and Rust keywords. It never checks whether two elements in `# peripherals`, `# devices`, or `# services` share an `id` (or collide after `rust_name()` replaces `-` with `_`, e.g., `my-led` vs `my_led`, or `to_uppercase()` for `__ID_STORAGE` statics, or PascalCase for `OledFactory`). Duplicate IDs pass `espforge check` and fail later in `rustc` with duplicate struct field or `static` errors.
* **`<i2c>` and `<spi>` never claim peripherals:** `peripherals/uart.rs` and `peripherals/wifi.rs` call `cx.claim_peripheral(...)`, catching duplicate `UARTn`/`WIFI` claims and runtime-held conflicts (`UART0`). `peripherals/i2c.rs` and `peripherals/spi.rs` never call `claim_peripheral`, so two `<i2c id="i2c0" ... />` declarations on different pins pass lowering silently and emit duplicate `__I2C0_STORAGE` statics and double hardware ownership.
* **Unreserved generated symbols:** `RESERVED_IDENTS` (`element.rs`) omits module-level and type-level identifiers emitted into `generated.rs`/`lib.rs` (`PeripheralRegistry`, `Devices`, `Services`, `LOG_FILTERS`, `LOGGER`, `__OTA_STORAGE`) and embassy `main()` locals like `timg0`.
* **`Devices::new` move vs. `clone_unchecked()` ordering:** `Devices::new` uses `unsafe { registry.peripherals.*.clone_unchecked() }`, while `main.rs` (`espforge-emit/src/codegen/rust.rs`) moves `TIMG0`, `FROM_CPU_INTR0`, `WIFI`, and `FLASH` out of `registry.peripherals` by value afterward. Any future device/peripheral that moves a field instead of cloning it—or uses `TIMG0` as a whole (since `hold_runtime_resources` only holds `TIMG0.timer0`)—will silently break `main.rs` compilation.

---

### 2. Split Truths & Parallel Validators That Drift

* **Three separate attribute validation paths:**
  1. `# espforge` (`espforge-elements/src/core.rs`) uses hand-written parsing and a manually maintained `KNOWN_ATTRS` array instead of `generated::validate`, duplicating `espf.schema.kdl`.
  2. Bundled elements (`espforge-elements/build.rs`) generate `Attrs` structs from the composed schema, but `build.rs` hard-panics on unimplemented `(AttrKind, required, default)` triples (e.g., `AttrKind::List`, `(Pin, false, Some(_))`, `(Str, true, Some(_))`) and emits `pub mod {tag}` verbatim, which breaks compilation if any tag contains `-`.
  3. External project drivers (`espforge-elements/src/dynamic_attrs.rs`) reinterpret `Element.attrs` dynamically—with subtle semantic divergences from `attrs.rs` (for example, `dynamic_attrs` treats `AttrKind::PinOrRef` as an unchecked pass-through, allowing empty strings `""` through until GPIO lookup fails).
  4. `PinRecipeElement` (`espforge-elements/src/devices/pin.rs`) bypasses `generated::validate` completely and never checks `Issue::Unknown`, so typos on `<led>` or `<button>` attributes (e.g., `activelow="true"`) are silently ignored and take the schema default.
* **Five disconnected chip-capability tables:** Chip support is spread across:
  * `esp-metadata-generated` (`chips::has_wifi`, `codegen/chip.rs`),
  * `chips.rs` (`NOT_YET_SUPPORTED`, plus hand-coded `i2c_instances`, `uart_instances`, `has_usb_serial_jtag`—for instance, `uart_instances` returns `3` for any non-C2/C3/C6/H2 chip, even though `esp32s2` has 2 UARTs, and guesses counts for `esp32c5`/`c61`/`s31`),
  * `pin_caps.rs` (manually committed copy from `esp-metadata-generated` 0.5.3),
  * `espforge-emit/src/codegen/wokwi.rs` (`board()` only maps 5 chips and silently falls back to `board-esp32-devkit-c-v4` for `esp32c2`, `esp32c5`, `esp32c61`, `esp32s31`),
  * `espforge/src/commands/doctor.rs` (`CHIPS` lists only 7 chips, missing `c5`/`c61`/`s31`),
  * `peripherals/spi.rs` (hardcodes `id.raw != "spi2"`, preventing `SPI3` on `esp32`/`s2`/`s3`).

---

### 3. Manifest & Recipe Pipeline Assumptions (`espforge-schema` & `devices/recipe.rs`)

* **String-length / span corruption after `env:NAME` substitution:** `espforge-core/src/substitutions.rs` replaces `attr.val` with `Cow::Owned(resolved)` while keeping `attr.val_offset` pointing at the original `env:NAME` token. Downstream (`espf/src/diag.rs::attr_span`, `espf/src/value.rs::insert`, `espforge-elements/src/attrs.rs::span_of`), spans are computed as `val_offset .. val_offset + val.len()`. Whenever the substituted value is longer than `"env:NAME"` and fails validation (e.g., `CORE_UNKNOWN_CHIP`, `COMPONENT_INVALID_SSID`, `DEVICE_INVALID_ID`), the span overruns into adjacent attributes or past EOF.
* **Unchecked `compose` literal bindings:** In `check_compositions` (`espforge-schema/src/lib.rs`), `compose` validates that a mapped attribute name exists on the part, **not** that `PartSource::Value(literal)` is valid for that attribute's `AttrKind`. Then `lower_recipe` (`devices/recipe.rs`) uses `.parse::<u8>().unwrap_or(0)` for `mode`, `hex_value` returning `None` for `address`, and `== "deferred"` for `init`—silently turning typos in `compose` blocks into wrong defaults instead of errors. Furthermore, `check_compositions` checks `part_recipe.bus != recipe.bus` without accounting for multi-transport `transports`.
* **`then` steps assume fallibility:** `step_statements` (`espforge-elements/src/model.rs`) unconditionally appends `.map_err(|_| DeviceError { ... })?;` to every `then { call ".." }` step. Any post-construction setup method returning `()` or `&mut Self` causes a `rustc` compile error in `generated.rs`.
* **Dual-mode fixed-address mismatch & memory leak in `driver.rs`:**
  * `parse_driver` (`espforge-schema/src/driver.rs`) calls `pin_alias(...).leak()`, leaking `'static` memory on every manifest parse.
  * `CtorSpec::shape` maps `Placeholder { name: "address" }` to `None`. If a dual-mode driver with a `fixed` address omits `{address}` in `blocking` and includes it in `async`, `blocking_args == async_args` passes, and `recipe.args` (taken only from `primary`) silently drops `{address}` on emission.
  * In `devices/recipe.rs::slots()`, `{address}` inside a wrapped fixed argument is spliced with raw `text.replace("{address}", ...)`, which can corrupt string literals inside the expression.
* **Multi-transport `package` flag asymmetry:** `parse_driver` computes `element.package` once using `recipe` (the first transport) and `BusDevice::emit` uses `driver.package.is_some()` as a proxy for "manifest-only vs. bundled driver" error mapping (`DeviceError` vs `DeviceErrorKind`).

---

### 4. Driver Lockfile, Path Containment, and Output Directory Lifecycle

* **`espforge gen` silently prunes unused drivers from `.espforge/drivers.lock`:** In `espforge-core/src/project.rs::generate_with`, after validating against the lockfile, `drivers::write_lock(dir, &drivers::record(&used))` overwrites `.espforge/drivers.lock` with *only* the drivers used in the current `.espf`. Temporarily commenting out `<my_driver />` and running `gen` deletes its lock entry—and for cached drivers, `drivers::collect` (`read_locked_cache`) only loads cached manifests present in the lockfile, so uncommenting `<my_driver />` fails instead of restoring it.
* **Relative path fragility in `espforge driver add`:**
  * `drivers::add` copies a `.kdl` file into `<project>/drivers/<tag>.kdl`, which invalidates relative `dependency { path ".." }` paths and triggers `resolve_package_paths` containment failures if the crate lives outside `project_dir`.
  * `drivers::add --local` stores the raw path string in `entry.source`; `drivers::update` and `verify_cache` later resolve it against the process's current working directory rather than `project_dir`.
* **Sibling `espforge-drivers` & stale `build/` artifacts:**
  * `--runtime-path /foo/espforge-runtime` (`espforge-emit/src/codegen/cargo.rs::drivers_dependency`) blindly assumes `/foo/espforge-drivers` exists at `RUNTIME_VERSION` without validating the directory.
  * `espforge-emit::write_all` writes generated files over `out` without removing stale artifacts from earlier runs (e.g., removing `<ota />` leaves an old `partitions.csv` in `build/`, and `overlay` / `check` skips validating `diagram.json` placeholders or `Cargo.extra.toml` TOML syntax during `espforge check`).

---

### 5. Runtime (`espforge-runtime`) Concurrency, Security, and Feature Coupling

* **Silent unverified TLS when `insecure="false"`:** Both `HttpsClient` (`espforge-runtime/src/services/https_client.rs`) and `WssClient` (`services/wss_client.rs`) store `self.insecure`, **never read it** during connection setup, and unconditionally construct `UnsecureProvider::new::<Aes128GcmSha256>(EspRng::new())`. Default `<https_client>` and `<wss_client>` configs accept any certificate without verification while appearing secure.
* **UART blocking/async lock-in & double-locking:**
  * Unlike `I2cBus` and `SpiBus` (which allow blocking buses on embassy projects), `UartDevice` gates `Blocking` vs `Async` exclusively via `#[cfg(feature = "embassy")]` (`hw/uart.rs`). An embassy project (e.g. using Wi-Fi) cannot instantiate a blocking `UartDevice` or blocking UART driver.
  * On embassy builds, `raw_bus.rs` wraps `UartDevice<Async>` inside `AsyncMutex<UartDevice<Async>>`, even though `UartDevice<Async>` already owns an internal `embassy_sync::mutex::Mutex`. Holding the outer `device!(ctx, uart1)` guard across `uart.read_line(...).await` (as `uart_example_async`'s `echo_task` loop does) starves any other task (or `setup()`) waiting on the outer `AsyncMutex`.
* **`!Sync` services on `Context`:** While devices (`Led`, `Button`, `I2cBus`, `SpiBus`) use `critical_section::Mutex` or `AsyncMutex` to remain `Send + Sync`, `WifiStation` (`hw/wifi.rs`) and `Clock` / `NtpClient` (`clock.rs`) still hold `RefCell`s, making `Context` `!Sync` and fragile if passed across multi-core / `Send`-task boundaries on chips like `esp32s3`.
* **Fragile external overlay placeholders:** `codegen/overlay.rs` only resolves `{{ <id>.pin }}` for `Device::Pin` (`<led>`, `<button>`), not for buses (`<i2c>`, `<spi>`, `<uart>`), `PinsDevice` (`<hx711>`, `<tm1637>`, `<dht22>`), or auxiliary pins on `BusDevice` (`<ili9341>`), forcing hardware diagrams to hardcode pin numbers that silently break when the `.espf` changes.

# Architecture Flaws & Scalability Bottlenecks

**Document Version:** 1.0.0  
**Scope:** `espforge` runtime, element lowering, driver abstraction, and code generation pipelines.

---

## Executive Summary

`espforge` successfully provides a fast scaffolding workflow for simple ESP32 `no_std` prototypes (such as a standalone blink, button, or single-sensor demo). However, the current architecture contains foundational design choices that **will fail if scaled to support dozens or hundreds of devices and real-world multi-peripheral IoT applications**.

If new drivers and components are added to `espforge-runtime` and `espforge-drivers` without restructuring, the project will hit dead ends requiring complete rewrites of the driver layer, the code generator, and the schema format.

The primary architectural bottlenecks are:
1. **The Runtime Dichotomy Trap:** Display/sensor drivers are hard-locked to blocking mode, while all networking services are hard-locked to Embassy async. You cannot build a device with both an OLED and Wi-Fi.
2. **The Monolithic Driver Crate Explosion:** Every driver is crammed into `espforge-drivers` behind individual feature flags, with raw strings parsed into Rust ASTs at runtime.
3. **Singleton State in `espforge-runtime`:** Static cells inside bus constructors trigger deterministic runtime panics whenever more than one bus instance is declared.
4. **The "Bus Stealing" Anti-Pattern:** Device drivers take buses out of `Components` using runtime `Option::take().expect(...)`, leaving dead `None` handles exposed to the user application.
5. **Reinventing Standard Ecosystem Bus Abstractions:** Custom `I2cBus`, `I2cDevice`, and `SpiDevice` wrappers bypass standard crates like `embedded-hal-bus`, preventing mixed-mode, multi-speed, and multi-slave bus sharing.
6. **Capability-Blind Pin Assignment:** The resource graph treats all GPIOs as identical integers, allowing output drivers to claim input-only pins or brick SPI Flash pins on boot.

---

## 1. The Runtime Dichotomy Trap (Blocking vs. Embassy Incompatibility)

### The Flaw
The architecture forces a mutually exclusive binary choice at the root of `# espforge`:
```xml
<runtime type="blocking" /> <!-- OR --> <runtime type="embassy" />
```

In `espforge-elements/src/devices/recipe.rs`:
```rust
// Hard rejection of devices under Embassy
if cx.runtime() != Runtime::Blocking {
    return Err(requires_blocking(cx, node, tag, &id, &recipe.kind));
}
```

In `espforge-elements/src/services/recipe.rs`:
```rust
// Hard rejection of services under Blocking
if recipe.requires_embassy && cx.runtime() != Runtime::Embassy {
    return Err(cx.report(codes::SERVICE_REQUIRES_EMBASSY, ...));
}
```

### Why It Prevents Scaling
* **The Connected Device Paradox:** The most common embedded IoT architecture is a device that connects to Wi-Fi/MQTT/HTTP and displays status on an OLED/TFT or reads from an I2C sensor.
* Under `espforge`'s current design:
  * Adding `<wifi_station>` or `<http_client>` **forces** `runtime="embassy"`.
  * Adding `<ssd1306>`, `<ili9341>`, or `<scd4x>` **forces** `runtime="blocking"`.
* **Result:** It is mathematically impossible in the current architecture to build an IoT display or internet-connected sensor station. If 200 drivers are written against this blocking convention, every one of them will have to be rewritten when async support is added.

### The Fix
Drivers must be written against generic `embedded-hal` (1.0) and `embedded-hal-async` traits or wrapped via synchronization adapters (like `embedded-hal-bus`), allowing Embassy async tasks to run synchronous driver operations via non-blocking executors or cooperative delays.

---

## 2. Singleton Memory Invariants & Multi-Instance Panics in `espforge-runtime`

### The Flaw
`espforge-runtime` uses function-scoped `static_cell::StaticCell` instances inside constructors:

```rust
// espforge-runtime/src/hw/i2c.rs
impl I2cBus<Blocking> {
    pub fn build<I: Instance + 'static>(...) -> Result<I2cBus<Blocking>, ...> {
        static CELL: static_cell::StaticCell<RefCell<I2c<'static, Blocking>>> =
            static_cell::StaticCell::new(); // <--- FUNCTION-LOCAL STATIC SINGLETON
        let inner = CELL.init(RefCell::new(esp)); // PANICS IF CALLED TWICE
        Ok(I2cBus { inner })
    }
}
```
The exact same pattern is replicated in:
* `I2cBus<Async>::build`
* `SpiBus<Blocking>::build`
* `SpiBus<Blocking>::build_write_only`
* `SpiBus<Async>::build`
* `SpiBus<Async>::build_write_only`
* `espforge-runtime/src/hw/wifi.rs` (`static RESOURCES: StaticCell<StackResources<4>>`)

### Why It Prevents Scaling
* ESP32, ESP32-S3, and ESP32-C6 chips feature multiple physical I2C and SPI peripherals (`I2C0`, `I2C1`, `SPI2`, `SPI3`).
* If a project defines:
  ```xml
  # peripherals
    <i2c id="i2c0" sda="6" scl="7" />
    <i2c id="i2c1" sda="4" scl="5" />
  ```
  Lowering accepts it. Codegen emits:
  ```rust
  let i2c0_bus = espforge_runtime::I2cBus::build(registry.peripherals.I2C0, ...);
  let i2c1_bus = espforge_runtime::I2cBus::build(registry.peripherals.I2C1, ...);
  ```
* At runtime, `i2c0_bus` initializes `CELL`. When `i2c1_bus` calls `CELL.init()`, `static_cell` triggers an **immediate runtime panic**.
* **Global Network Contention:** `HttpClient` uses a single static mutex:
  ```rust
  // espforge-runtime/src/services/http_client.rs
  static SHARED: Mutex<CriticalSectionRawMutex, Shared> = Mutex::new(...);
  ```
  All HTTP requests in the system share one 4096-byte static buffer. Two concurrent tasks making HTTP calls block and starve each other across `.await` points.

### The Fix
Storage must be declared in generated code per instance (in `generated.rs`), not as hidden singletons inside `espforge-runtime`. Handles in `espforge-runtime` should borrow `&'static` references passed into their constructors.

---

## 3. The "Bus Stealing" (`Option::take`) Anti-Pattern & Macro Incoherence

### The Flaw
When a device needs exclusive ownership of a bus handle (such as an SPI display), `espforge` uses runtime mutation across struct boundaries:

```rust
// 1. espforge-emit generates a struct with an Option:
pub struct Components {
    pub tft_bus: Option<espforge_runtime::SpiDevice<esp_hal::Blocking>>,
}

// 2. In main.rs, Devices::new mutates Components and steals it:
let mut components = Components::new(&mut registry);
let devices = Devices::new(&mut registry, &mut components);

// 3. Inside Devices::new:
tft_bus: components.tft_bus.take().expect("the display's SPI bus was already taken")
```

To accommodate this mutation, `src/lib.rs` switches the borrow mode of the `device!` macro globally:
```rust
// espforge-emit/src/codegen/rust.rs:lib()
let device_borrow = if has_devices { quote! { &mut } } else { quote! { & } };
```

### Why It Prevents Scaling
* **The Zombie Field Anti-Pattern:** Once `Devices::new` completes, `ctx.components.tft_bus` still exists in the user's namespace, but its value is permanently `None`. Any application code calling `ctx.components.tft_bus.as_ref().unwrap()` crashes at runtime.
* **Borrow Checker Gridlock:** When `device!` returns `&mut $ctx.devices.$name`, user code cannot access two devices simultaneously in the same function:
  ```rust
  let display = device!(ctx, tft);
  let sensor = device!(ctx, scd40); // FAILS: Cannot borrow `ctx.devices` as mutable more than once
  ```
* **Deferred Runtime Failures:** Misconfigured bus sharing is converted from a compile-time error into a runtime string panic (`.expect("the display's SPI bus was already taken")`).

### The Fix
Adopt staged construction. Components that are internal bus endpoints of devices must **not** be exposed as public fields on `Components`. `Devices` should construct and own its bus adapters directly.

---

## 4. Reinventing Bus Sharing Instead of Using `embedded-hal-bus`

### The Flaw
`espforge-runtime` manually implements bus sharing and device wrappers:
* `I2cDevice` wraps `I2cBus` in a `RefCell` or `Mutex` and overrides the address argument.
* `SpiDevice` manually manages a `RefCell<Output<'static>>` for Chip Select and bit-bangs CS lines:
  ```rust
  // espforge-runtime/src/hw/spi.rs
  cs.set_low();
  let result = (|| { ... operations ... })();
  cs.set_high();
  ```

### Why It Prevents Scaling
* **Locked Bus Configurations:** On a real-world SPI bus, different devices require different modes and clock frequencies:
  * Device A (e.g. Flash memory): SPI Mode 0, 40 MHz.
  * Device B (e.g. Display controller): SPI Mode 3, 10 MHz.
  * Device C (e.g. Sensor): SPI Mode 0, 1 MHz.
* In `espforge`, the clock frequency and mode are **locked globally to `<spi>`**:
  ```xml
  <spi id="spi2" sclk="8" mosi="7" mode="0" frequency="40000kHz" />
  ```
  Every device on that bus is forced to run at the exact same mode and clock speed. A 1 MHz sensor on the same bus will receive 40 MHz pulses and corrupt data.
* **Lack of CS Customization:** The implementation cannot support active-high Chip Select, software delays between CS assertion and clocking, or multi-device transactions.
* **Ecosystem Incompatibility:** Community device drivers expect standard `embedded-hal` bus sharing implementations (`embedded-hal-bus::spi::RefCellDevice`, `CriticalSectionDevice`).

### The Fix
Deprecate the custom `SpiDevice`/`I2cDevice` implementations in `espforge-runtime` and standardize on `embedded-hal-bus`.

---

## 5. Stringly-Typed Driver Recipes & Runtime AST Parsing

### The Flaw
The device and service architectures rely on string templates in KDL files parsed into Rust tokens at runtime during codegen:

```kdl
// drivers.schema.kdl
element "ili9341" section="devices" {
    device_recipe {
        type "espforge_drivers::Ili9341Display"
        new "espforge_drivers::Ili9341Display::new"
        arg "dc" pin="esp_hal::gpio::Output::new(unsafe { registry.peripherals.GPIO{n}.clone_unchecked() }, esp_hal::gpio::Level::Low, esp_hal::gpio::OutputConfig::default())" direction="output"
        arg "rotation" {
            map "0" "espforge_drivers::Orientation::Portrait"
            map "90" "espforge_drivers::Orientation::Landscape"
        }
    }
}
```

In `espforge-elements/src/devices/recipe.rs`:
```rust
let expression = template.replace("{n}", &pin.pin().0.to_string());
// Later in Device::emit:
let ty: TokenStream = self.ty.parse().expect("...");
let new: TokenStream = self.new.parse().expect("...");
let args: Vec<TokenStream> = self.args.iter().map(|arg| arg.parse().expect("...")).collect();
```

### Why It Prevents Scaling
* **Zero Syntax Validation at Rest:** Raw Rust code strings are stored inside KDL schema files. A syntax error, missing bracket, or API change in `esp-hal` inside a KDL string is only discovered when `syn::parse2` runs during code generation.
* **Brittle Code Injection Anchor Matching:** In `espforge-emit/src/codegen/comments.rs`:
  ```rust
  // Searches text line by line for anchor string, and injects "// comment"
  let matches: Vec<usize> = text.lines().enumerate()
      .filter(|(_, line)| line.contains(&comment.anchor))
      .map(|(index, _)| index).collect();
  if matches.len() != 1 {
      bail!("comment anchor `{}` matched {} lines, expected exactly one", comment.anchor, matches.len());
  }
  ```
  Safety comments are inserted into generated code by matching substring anchors (e.g. `anchor: "red_led: espforge_runtime::Led::new("`).
  If `prettyplease` wraps a constructor across lines, or if two devices share similar initialization calls, `matches.len() != 1` triggers, and project generation aborts.

### The Fix
Replace string templating in KDL with typed AST code generation modules in Rust or macro-based driver registration where compiler types validate the emitted code.

---

## 6. Monolithic `espforge-drivers` Crate & Feature Flag Explosion

### The Flaw
All device drivers live inside a single crate: `espforge-drivers`.

```toml
# espforge-drivers/Cargo.toml
[dependencies]
ssd1306 = { version = "0.9", optional = true }
sky-ili9341 = { version = "0.1.1", optional = true }

[features]
ssd1306 = ["dep:ssd1306", "dep:embedded-graphics"]
ili9341 = ["dep:sky-ili9341", "dep:embedded-graphics"]
scd4x = []
```

And in `espforge-emit/src/codegen/cargo.rs`:
```rust
// Sibling path inference
let path = std::path::Path::new(path)
    .parent()
    .map(|parent| parent.join("espforge-drivers"))
```

### Why It Prevents Scaling
* **Dependency Graph Nightmare:** When scaling to 100+ drivers:
  * `espforge-drivers/Cargo.toml` will contain hundreds of optional dependencies.
  * Incompatible transitive dependencies (e.g. Driver X requires `embedded-hal 0.2` via `embedded-hal-compat`, Driver Y requires `embedded-hal 1.0`, Driver Z requires an older `heapless`) will cause dependency resolution failures or feature unification conflicts across the workspace.
* **Closed Ecosystem:** Users cannot import third-party drivers or write custom drivers in their own crates because the compiler pipeline only knows how to generate dependencies pointing to `espforge-drivers`.
* **Fragile Path Assumption:** Setting `--runtime-path /some/path/espforge-runtime` unconditionally forces `Cargo.toml` to link to `/some/path/espforge-drivers`. If `espforge-drivers` is not at that relative path, compilation fails.

### The Fix
Driver recipes should allow specifying arbitrary crates and versions in the schema:
```kdl
device_driver "bme280" {
    crate "bme280-rs" version="0.3"
    constructor "bme280_rs::Bme280::new"
}
```
`espforge` should generate direct dependencies in the target's `Cargo.toml` rather than routing everything through a monolithic wrapper crate.

---

## 7. Capability-Blind Hardware Pin Allocation (Flash & Input-Only Pin Hazard)

### The Flaw
The `ResourceGraph` in `espforge-elements/src/model.rs` models pins purely as numbers:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pin(pub u8);
```
It tracks whether two elements collide on the same pin number or have opposing `PinUse` (Input vs. Output), but **it has no model of the physical hardware capabilities of those pins on the target chip**.

### Why It Prevents Scaling
Every ESP32 variant has strict pin constraints that are currently unvalidated:
* **Input-Only Pins:** On the ESP32, GPIO 34, 35, 36, and 39 are input-only. They have no output drivers or internal pull-up/pull-down resistors.
  * A spec declaring `<gpio id="led_pin" pin="34" direction="output" />` and `<led id="status" gpio="$led_pin" />` passes validation cleanly.
  * `esp-hal` will fail to compile or panic at runtime when attempting to configure it as an `Output`.
* **SPI Flash & PSRAM Corruption:** On the ESP32 and ESP32-S3, GPIO 6 through 11 are directly wired to the SPI flash bus.
  * An author can declare `<gpio id="bad" pin="6" direction="output" />`.
  * `espforge` will generate `unsafe { registry.peripherals.GPIO6.clone_unchecked() }`.
  * The moment the firmware runs, toggling GPIO 6 disrupts cache access to flash memory, immediately causing a **CPU Fatal Exception / Crash Loop**.
* **Strapping Pin Traps:** GPIO 0, 2, 8, 9, 12, 15 (depending on chip) control boot modes. Driving them without warnings causes boards to fail flashing or boot into wrong download modes.

### The Fix
Query `esp-metadata-generated` during `LowerCx::claim_pin` to cross-reference pin numbers against chip-specific pin capability bitmaps:
```rust
// Reject output claims on input-only pins
if need == PinUse::Output && !chip_info.is_output_capable(pin) {
    return Err(codes::GPIO_NOT_OUTPUT_CAPABLE);
}
// Reject claims on SPI Flash / RAM pins
if chip_info.is_flash_pin(pin) {
    return Err(codes::GPIO_RESERVED_FLASH);
}
```

---

## 8. Inflexible Monolithic Initialization & Panic-on-Boot Drivers

### The Flaw
All device drivers are initialized synchronously inside a single monolithic constructor in `src/generated.rs`:

```rust
// src/generated.rs
impl Devices {
    pub fn new(registry: &mut PeripheralRegistry, components: &mut Components) -> Self {
        Self {
            oled: espforge_drivers::Ssd1306Display::new(...),
            tft: espforge_drivers::Ili9341Display::new(...),
            air: espforge_drivers::Scd4x::new(...),
        }
    }
}
```
And inside the driver constructors (e.g. `espforge-drivers/src/ssd1306.rs`):
```rust
display.init().expect("SSD1306 OLED initialization failed");
```

### Why It Prevents Scaling
* **Zero Fault Tolerance:** If a sensor or display is disconnected, has a loose wire, or experiences a transient I2C NACK on boot, `.expect()` panics the CPU. The device goes into an infinite bootloop and never reaches `app::setup()` or brings up Wi-Fi diagnostics.
* **No Dynamic Power Management:** All devices are initialized at power-on. A battery-powered application cannot defer turning on a sensor or display until needed.
* **Monolithic Macro Borrowing:**
  ```rust
  #[macro_export]
  macro_rules! device {
      ($ctx:expr, $name:ident) => {
          &mut $ctx.devices.$name
      };
  }
  ```
  Because `device!(ctx, name)` borrows the whole `ctx.devices` struct mutably, calling helper functions that need access to different devices triggers borrow-checker errors.

### The Fix
1. Device initialization should return `Result<Self, DeviceError>`.
2. Allow `Devices::new` to produce initialized handles or allow `app::setup` to initialize/reconnect devices on demand.
3. Accessors must borrow individual fields rather than the parent struct container.

---

## 9. Non-Transactional Lowering State Machine

### The Flaw
Lowering mutates `ResourceGraph` incrementally across multiple steps without rollback capabilities:

```rust
// espforge-elements/src/peripherals/uart.rs
cx.claim_peripheral(peripheral.clone(), owner.clone(), span.clone())?;
let tx = claim_bus_pin(cx, attrs.tx.value, owner.clone(), span.clone())?;
let rx = claim_bus_pin(cx, attrs.rx.value, owner, span)?; // <--- If this fails...
cx.add_bus(...); // Never called
```

In `espforge-elements/src/lower.rs`:
```rust
lower_section("peripherals", peripherals, &mut cx);
check_bus_gpio_declarations(&mut cx);
if cx.has_errors() {
    return cx.into_board(meta);
}
```

### Why It Prevents Scaling
* If `rx` fails, `tx` remains recorded in `cx.resources.pins`, and `peripheral` remains recorded in `cx.resources.peripherals`.
* When lowering attempts to continue to collect all diagnostics, subsequent components or peripherals evaluated against those pins fail with misleading cascading errors.
* The system violates the ACID principle of compiler frontend passes: a failed syntactic element must not leave dirty mutations in the symbol and resource tables.

### The Fix
Implement a transactional rollback guard (`ResourceTransaction`) that stages pin and peripheral claims and commits them only when the entire element validates cleanly.

---

## 10. Strict Manifest Drift Locks Users Out of Cargo Ecosystem

### The Flaw
In `espforge-emit/src/manifest.rs` and `lib.rs`, `Cargo.toml` is marked as `Ownership::Owned`:

```rust
// If user modified Cargo.toml:
if manifest::checksum(&existing) != *prev_sum && existing != a.content {
    return Err(ManifestDrift { path: a.path.clone() }.into());
}
```

### Why It Prevents Scaling
* As a project grows, users **must** add crates that `espforge` does not provide built-in elements for (e.g. `serde_json`, math libraries, custom protocol decoders, cryptographic primitives, special drivers).
* Modifying `Cargo.toml` breaks the FNV-1a checksum.
* Running `espforge build` then permanently aborts:
  ```
  ❌ refusing to overwrite edited file `Cargo.toml` (drift detected). Restore it or delete it to regenerate.
  ```
* Users are forced to abandon `espforge build` entirely and run raw `cargo build`, completely breaking the `.espf` workflow for downstream project evolution.

### The Fix
1. Provide an `[extra_dependencies]` section in `.espf` or a `.cargo/user-dependencies.toml` file that is merged into `Cargo.toml` on every build.
2. Change `Cargo.toml` to a managed merge model (using `toml_edit` to preserve user-added dependencies while updating `espforge`-managed dependencies).

---

## Summary Comparison: Current Architecture vs. Scalable Architecture

| Subsystem | Current Implementation | Failure Mode at Scale | Scalable Architecture |
|---|---|---|---|
| **Runtime Selection** | Binary blocking vs. embassy switch per project | Cannot combine networking services with displays or sensors | Uniform async runtime with blocking adapters via `embedded-hal-bus` |
| **Hardware State** | Function-local `static_cell::StaticCell` singletons | Deterministic panic on boards with $>1$ I2C or SPI bus | Storage allocated per-instance in `generated.rs` |
| **Bus Sharing** | Custom `RefCell` and manual CS pin bit-banging | Cannot support mixed-mode or mixed-frequency SPI buses | Standardized on `embedded-hal-bus` |
| **Driver Sourcing** | Monolithic `espforge-drivers` crate with optional features | Cargo dependency hell; locked to bundled drivers | Direct crate & version injection into `Cargo.toml` |
| **Pin Validation** | Integer-only tracking; checks duplicate claims only | Allows outputs on input-only pins; bricks SPI Flash pins | Validation against `esp-metadata` chip capability bitmaps |
| **Bus Ownership** | `Option::take().expect(...)` across struct boundary | Zombie `None` fields on Context; runtime panics on unwrap | Direct structural dependency injection in constructor tiers |
| **Device Init** | Synchronous `.expect(...)` in generated code | Unplugged sensor triggers infinite bootloop | Fallible `init()` returning `Result`, with retry support |
| **Lowering Engine** | Incremental dirty mutation of `ResourceGraph` | Phantom pin conflicts and misleading diagnostic cascades | Atomic transactional claims with rollback on failure |
| **Project Evolution** | Rigid checksum drift detection on `Cargo.toml` | Adding custom crates locks user out of `espforge build` | Semantic TOML merging preserving user dependencies |

Based on a forensic architectural analysis of the workspace, here are the **top 3 highest-priority architectural improvements**. These address critical design flaws that cause runtime panics on valid specifications, broken invariants during lowering, and brittle ownership models between generated structs.

---

### Priority 1: Multi-Instance State Architecture in `espforge-runtime`
*Eliminate singleton `StaticCell` panics and shared global state.*

#### The Architectural Flaw
In `espforge-runtime/src/hw/i2c.rs`, `spi.rs`, and `services/http_client.rs`, hardware buses and service clients rely on **function-local or global static singletons**:

```rust
// espforge-runtime/src/hw/i2c.rs
impl I2cBus<Blocking> {
    pub fn build<I: Instance + 'static>(...) -> Result<I2cBus<Blocking>, ...> {
        static CELL: static_cell::StaticCell<RefCell<I2c<'static, Blocking>>> =
            static_cell::StaticCell::new(); // <--- LOCAL STATIC SINGLETON!
        let inner = CELL.init(RefCell::new(esp)); // PANICS if called more than once!
        Ok(I2cBus { inner })
    }
}
```
* **Failure Mode 1 (Deterministic Runtime Panic):** The ESP32-S3 and ESP32-C6 support multiple hardware I2C controllers (`I2C0`, `I2C1`). If an author defines two buses in `.espf` (e.g. `i2c0` and `i2c1`), `Components::new()` calls `I2cBus::build()` twice. The second call invokes `StaticCell::init()` on an already-initialized cell, causing an **immediate hard panic on boot**. The exact same bug exists in `SpiBus::build` and `SpiBus::build_write_only`.
* **Failure Mode 2 (Concurrency Bottleneck & Deadlock):** `HttpClient` routes all requests through a single global static `SHARED: Mutex<CriticalSectionRawMutex, Shared>` with a hard-coded 4 KB buffer. Two concurrent tasks calling `.get()` will block each other across awaits, serializing network I/O and starving Embassy executors.

#### The Architectural Redesign
Invert storage ownership: **the generated firmware crate (`generated.rs`) must own the storage cells**, while `espforge-runtime` provides un-opinionated handles borrowing `&'static` storage provided to them.

```
[Current (Broken)]
generated.rs ──► I2cBus::build() ──► Internal static CELL (Panics on 2nd bus!)

[Target Architecture]
generated.rs:
  static I2C0_CELL: StaticCell<...>
  static I2C1_CELL: StaticCell<...>
        │
        └──► I2cBus::new(I2C0_CELL.init(...))  <── 100% Multi-instance Safe
```

#### Implementation Blueprint

1. **Refactor `I2cBus` and `SpiBus` constructors in `espforge-runtime`:**
```rust
// espforge-runtime/src/hw/i2c.rs
impl<Dm: DriverMode + 'static> I2cBus<Dm> {
    pub fn from_raw(inner: &'static MutexCell<I2c<'static, Dm>>) -> Self {
        Self { inner }
    }
}
```

2. **Emit instance-dedicated `StaticCell`s in `espforge-emit/src/codegen/rust.rs`:**
```rust
// Generated into src/generated.rs for each bus
static I2C0_BUS_CELL: static_cell::StaticCell<core::cell::RefCell<esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>> = static_cell::StaticCell::new();
static I2C1_BUS_CELL: static_cell::StaticCell<core::cell::RefCell<esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>> = static_cell::StaticCell::new();
```

---

### Priority 2: Transactional Lowering & Symbol Table Isolation in `espforge-elements`
*Enforce atomic pin reservation and prevent codegen identifier collisions.*

#### The Architectural Flaw
1. **Non-Transactional Resource Allocation (`LowerCx`):**
   In `espforge-elements/src/peripherals/uart.rs` (and similarly in `i2c.rs` and `spi.rs`):
   ```rust
   cx.claim_peripheral(peripheral.clone(), owner.clone(), span.clone())?;
   let tx = claim_bus_pin(cx, attrs.tx.value, owner.clone(), span.clone())?;
   let rx = claim_bus_pin(cx, attrs.rx.value, owner, span)?; // <--- If this fails...
   // cx.add_bus(...) is never reached!
   ```
   If `rx` triggers a pin conflict, `tx` and `peripheral` have already been inserted into `cx.resources.pins` and `cx.resources.peripherals`. The claim is **never rolled back**. When subsequent diagnostics run or subsequent elements check ownership, they collide with orphaned claims from a ghost bus that was never constructed.

2. **Unchecked Generated Identifier Collisions:**
   `LowerCx::ident()` only checks if the user's string is a valid generic Rust ident (`is_rust_ident`). It does not validate against system bindings declared in `MainCtx` (`main.rs`) or language keywords:
   * **Collision targets:** `registry`, `peripherals`, `logger`, `delay`, `components`, `devices`, `services`, `ctx`, `spawner`, `app`.
   * **Failure Mode:** If a user defines `<led id="components" gpio="$gpio18" />` or `<wifi_station id="spawner" ... />`, `main.rs` emits:
     ```rust
     let mut components = Components::new(&mut registry);
     components.components.toggle(); // or shadowing 'spawner' before wifi_station.start()
     ```
     This triggers obscure downstream `rustc` compiler errors rather than clean, localized validation diagnostics.

#### The Architectural Redesign
1. **Transactional Unit of Work Pattern:** Implement a `Transaction` boundary on `ResourceGraph`. Claims are staged in a rollback journal and only committed if the entire peripheral/component lowers cleanly.
2. **Unified Semantic Symbol Table:** Maintain a reserved keyword table in `LowerCx` checking Rust 2024 keywords and generated `MainCtx` symbols at parse time.

#### Implementation Blueprint

```rust
// In espforge-elements/src/model.rs
impl ResourceGraph {
    pub fn begin_transaction(&mut self) -> ResourceTransaction<'_> {
        ResourceTransaction {
            graph: self,
            staged_pins: Vec::new(),
            staged_peripherals: Vec::new(),
        }
    }
}

pub struct ResourceTransaction<'a> {
    graph: &'a mut ResourceGraph,
    staged_pins: Vec<Pin>,
    staged_peripherals: Vec<String>,
}

impl<'a> ResourceTransaction<'a> {
    pub fn claim_pin(&mut self, pin: Pin, owner: Owner, span: Range<usize>, need: PinUse) -> Result<ClaimedPin, Conflict> {
        let claimed = self.graph.claim(pin, owner, span, need)?;
        self.staged_pins.push(pin);
        Ok(claimed)
    }

    pub fn commit(self) {
        // Disarm drop guard so claims persist
        core::mem::forget(self);
    }
}

impl<'a> Drop for ResourceTransaction<'a> {
    fn drop(&mut self) {
        // Rollback any partially claimed resources on error/early return
        for pin in self.staged_pins.drain(..) {
            self.graph.pins.remove(&pin);
        }
        for periph in self.staged_peripherals.drain(..) {
            self.graph.peripherals.remove(&periph);
        }
    }
}
```

```rust
// In espforge-elements/src/element.rs
pub const RESERVED_SYSTEM_IDENTS: &[&str] = &[
    "registry", "peripherals", "logger", "delay", "components",
    "devices", "services", "ctx", "spawner", "app", "main", "setup", "forever",
];

pub fn validate_ident(cx: &mut LowerCx, node: &ParsedNode<'_>, id: &Ident) -> Result<(), Reported> {
    if RESERVED_SYSTEM_IDENTS.contains(&id.rust_name().as_str()) || syn::parse_str::<syn::Ident>(&id.rust_name()).is_err() {
        return Err(cx.report(
            codes::COMPONENT_INVALID_ID,
            id.span.clone(),
            "reserved or invalid identifier",
            format!("'{}' cannot be used as an id because it is a reserved system or language binding", id.raw),
        ));
    }
    Ok(())
}
```

---

### Priority 3: Eliminate Runtime "Bus Stealing" with Staged Construction
*Replace runtime `Option::take().expect(...)` with structural ownership.*

#### The Architectural Flaw
In `espforge-elements/src/devices/recipe.rs` and `espforge-emit/src/codegen/rust.rs`, when a display device declares `takes_bus` (e.g. `<ili9341>` taking `<spi_component>` by value):

1. Codegen mutates the bus component field in `Components` to an `Option`:
   ```rust
   pub struct Components {
       pub tft_bus: Option<espforge_runtime::SpiDevice<esp_hal::Blocking>>,
   }
   ```
2. In `main.rs`, `Devices::new` runs *after* `Components::new` and takes the bus:
   ```rust
   let mut components = Components::new(&mut registry);
   let devices = Devices::new(&mut registry, &mut components);
   // Inside Devices::new:
   tft_bus: components.tft_bus.take().expect("the display's SPI bus was already taken")
   ```
3. To compensate for `Devices` mutation, `src/lib.rs` switches the `device!` macro expansion globally:
   ```rust
   let device_borrow = if has_devices { quote! { &mut } } else { quote! { & } };
   ```

* **Why this is bad architecture:**
  * **Hollow Shell Anti-Pattern:** After initialization, `ctx.components.tft_bus` remains permanently accessible to `app.rs`, but is silently `None`. Calling `.unwrap()` in user code results in a runtime crash.
  * **Fragile Macro Couplings:** `device!(ctx, ...)` changes its borrow semantics from immutable `&` to mutable `&mut` across the entire crate based on a global boolean `!board.devices.is_empty()`.
  * **Unnecessary Deferred Panics:** Bus ownership conflicts are deferred to runtime string panics (`.expect(...)`) instead of being enforced by structural typestates during generation.

#### The Architectural Redesign
Construct resources in topological dependency order across distinct initialization tiers. An SPI device intended for a display should be initialized as an **anonymous intermediate or directly owned by `Devices`**, never staged as a zombie `Option` in `Components`:

```
[Current (Runtime Stealing)]
Components::new() ──► Stores Some(SpiDevice)
                               │
Devices::new() ◄───────────────┘ (.take() leaves None in Components!)

[Target Architecture (Staged Initialization)]
Main:
  1. Registry::new(peripherals)
  2. Components::new(&mut registry) ──► only contains standalone components (LEDs, Buttons)
  3. Devices::new(&mut registry)     ──► directly constructs SPI controller + Display
     (or passes dedicated BusHandle without exposing hollow Options on Context)
```

#### Implementation Blueprint

In `espforge-emit/src/codegen/rust.rs`:
1. If a component is marked `takes_bus`, do **not** declare it as a public member of `pub struct Components`.
2. Construct the bus controller directly in `main.rs` or inside `Devices::new`:
   ```rust
   // In src/generated.rs
   pub struct Components {
       pub red_led: espforge_runtime::Led,
       // tft_bus is NOT here: it is not a standalone component!
   }

   pub struct Devices {
       pub tft: espforge_drivers::Ili9341Display,
   }

   impl Devices {
       pub fn new(registry: &mut PeripheralRegistry) -> Self {
           let tft_bus = espforge_runtime::SpiBus::build_write_only(...);
           let tft_device = espforge_runtime::SpiDevice::new(tft_bus, ...);
           Self {
               tft: espforge_drivers::Ili9341Display::new(tft_device, ...),
           }
       }
   }
   ```
3. `ctx.components` now only contains valid, live components. No `Option::take()`, no runtime panics, and zero zombie fields.

---

### Summary Matrix of Improvements

| Priority | Targeted Subsystem | Root Problem | Impact After Refactor |
|---|---|---|---|
| **1** | `espforge-runtime` | Singleton `StaticCell` in `hw/{i2c,spi}.rs` panics on boards with $>1$ bus. | Multi-bus boards (`i2c0` + `i2c1`) work reliably without runtime boot panics. |
| **2** | `espforge-elements` | Non-transactional pin claiming in `LowerCx` + unreserved system identifiers. | Atomic lowering rollback prevents phantom pin conflicts; compiler namespace collisions are blocked with clean diagnostics. |
| **3** | `espforge-emit` | `Option::take().expect(...)` bus stealing leaves empty `None` fields on `Context`. | Clean structural ownership; dead `None` components are eliminated from the app's autocompletion and API surface. |

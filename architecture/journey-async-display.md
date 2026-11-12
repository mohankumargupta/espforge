# The Journey of `ssd1306_async`: An Async Panel on a Deferred Bus

This is the phase-6 walkthrough: a project whose panel is built **after** boot
and whose transfers **await** the bus. It follows the same spec →
`generated.rs` → `app.rs` path as `blink`, with the device tier doing the work.

---

## 1. The Spec

```xml
# espforge
  <project name="ssd1306_async" />
  <chip type="esp32c3" />
  <runtime type="embassy" />

# peripherals
  <i2c id="i2c0" sda="6" scl="7" frequency="400kHz" />

# devices
  <ssd1306 id="oled" bus="$i2c0" address="0x3C" size="128x64" />

# services
```

Three decisions meet here:

- `<runtime type="embassy">` chooses the scheduler, as in phase 4.
- `<ssd1306>` is a **dual-mode** driver. Its recipe declares a blocking type
  and a `runtime "async"` block; on an embassy project the bus resolves async
  and the async block is the one emitted.
- An async device is always **deferred**: `Devices::new` is synchronous and
  runs before the executor, so the constructor is stored as a factory and the
  app awaits `init()`.

`espforge check` accepts this project and `espforge gen` writes it with the
driver feature `ssd1306-async`, which turns on `espforge-runtime/embassy` and
`ssd1306/async`.

---

## 2. `src/generated.rs`

The bus is built before the executor starts; the panel is not:

```rust
static __I2C0_STORAGE: espforge_runtime::AsyncI2cBusStorage =
    espforge_runtime::AsyncI2cBusStorage::new();

/// Built during `Devices::new`, initialised after boot.
pub struct OledFactory {
    endpoint: espforge_drivers::AsyncI2cEndpoint,
}

impl espforge_runtime::AsyncDeferredInit for OledFactory {
    type Device = espforge_drivers::AsyncSsd1306Display<espforge_drivers::Oled128x64>;

    fn init(self) -> impl core::future::Future<Output = Result<Self::Device, espforge_runtime::DeviceError>> {
        async move {
            espforge_drivers::AsyncSsd1306Display::new(
                    self.endpoint,
                    60,
                    espforge_drivers::Oled128x64,
                    espforge_drivers::DisplayRotation::Rotate0,
                )
                .await
                .map_err(|kind| espforge_runtime::DeviceError { device: "oled", kind })
        }
    }
}

pub struct Devices {
    pub oled: espforge_runtime::AsyncMutex<espforge_runtime::AsyncDeferred<OledFactory>>,
}

impl Devices {
    pub fn new(registry: &mut PeripheralRegistry) -> Result<Self, espforge_runtime::DeviceError> {
        let i2c0_bus: espforge_runtime::AsyncI2cBus = espforge_runtime::AsyncI2cBus::build_in(
                &__I2C0_STORAGE,
                unsafe { registry.peripherals.I2C0.clone_unchecked() },
                unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                unsafe { registry.peripherals.GPIO7.clone_unchecked() },
                espforge_runtime::I2cConfig {
                    frequency: esp_hal::time::Rate::from_khz(400),
                },
            )
            .expect("I2C bus configuration failed");
        Ok(Self {
            oled: espforge_runtime::AsyncMutex::new(
                espforge_runtime::AsyncDeferred::new(OledFactory { endpoint: i2c0_bus }),
            ),
        })
    }
}
```

Things to notice:

1. `I2cBusStorage` never appears: the async project builds an `AsyncI2cBus`,
   whose storage is a mutex-armoured cell.
2. The factory holds only the endpoint and the constants the constructor needs
   (address, size, rotation). No bus traffic happens in `Devices::new`.
3. The field is an `AsyncMutex`, not a `RefCell`: the guard is `Send`-free but
   may be held across `.await`, which a `RefCell` borrow may not.
4. The factory's future is **not** declared `Send`. esp-hal's async bus
   handles contain `PhantomData<*const ()>`, so a `Send` bound would reject
   every real panel; the entry point awaits `init` before spawning anything.

---

## 3. `src/lib.rs`

```rust
#[macro_export]
macro_rules! async_device {
    ($ctx:expr, $name:ident) => {
        $ctx.devices.$name.lock().await
    };
}
```

`device!` still exists and expands to `borrow_mut()`; on an async device that
is a compile error, which is the intended nudge towards `async_device!`.

---

## 4. `src/app.rs`

```rust
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    let mut slot = async_device!(ctx, oled);
    let oled = match slot.init().await {
        Ok(oled) => oled,
        Err(error) => {
            ctx.logger.error(format_args!("SSD1306 init failed: {error}"));
            return;
        }
    };

    Text::new("ESPFORGE ASYNC", Point::new(36, 20), text_style)
        .draw(oled)
        .ok();
    if let Err(()) = oled.flush().await {
        ctx.logger.warn("SSD1306 flush failed");
    }
}
```

`init()` runs the constructor and its I2C probe the first time it is awaited;
a later call returns the same device. The guard lives across both `.await`
points, which is safe because it is a mutex guard rather than a `RefCell`
borrow.

---

## 5. Boot and build

- `main` builds the async bus and the factory, then starts `esp_rtos`; the
  panel's first transfer happens in `setup`.
- `espforge gen --example 06.Displays/ssd1306_async` writes the project;
  `cargo build --target riscv32imc-unknown-none-elf` compiles it.
- `ssd1306` 0.9 expands its async impls with `maybe-async-cfg`; the driver
  feature pins that crate to 0.2.4 because 0.2.5 mis-expands them.

---

## 6. The guard rule is a test

The rule that a `RefCell` borrow must not be held across `.await` is no longer
documentation only: `espforge/tests/compile_fail.rs` generates a project,
breaks one line, and asserts the compiler's complaint. It is `#[ignore]` like
the golden compile gate and runs in the release gate:

```sh
cargo test -p espforge --test compile_fail -- --ignored
```

# The Journey of `ssd1306_wifi`: A Display and a Radio in One Firmware

This is the phase-5 walkthrough: the same spec → `generated.rs` → `app.rs` path
as `blink`, for a project that needs an embassy scheduler **and** a blocking I2C
panel.

---

## 1. The Spec

```xml
# espforge
  <project name="ssd1306_wifi" />
  <chip type="esp32c3" />
  <runtime type="embassy" />

# peripherals
  <i2c id="i2c0" sda="6" scl="7" frequency="400kHz" />
  <wifi id="radio" />

# devices
  <ssd1306 id="oled" bus="$i2c0" address="0x3C" size="128x64" />

# services
  <wifi_station id="home" wifi="$radio" ssid="Wokwi-GUEST" auth="open" />
```

Three separate decisions meet here:

- `<runtime type="embassy">` chooses the **scheduler**: `main` is async and the
  station spawns tasks.
- `<wifi id="radio">` is a peripheral, so `${radio}` is a real radio the
  station can start.
- `<ssd1306>` declares a **blocking** driver on `i2c0`. The bus mode is derived
  from its drivers, so `i2c0` is a blocking bus even though the project is an
  embassy project. That is phase 4's rule; phase 5 makes the handle usable.

`espforge check` accepts this project. Before phase 4 it was refused with
`E0906` because every device was blocking-only and every network service was
embassy-only.

---

## 2. `src/generated.rs`

The bus and the display are built before the executor starts, which is the only
blocking work on the board:

```rust
static __I2C0_STORAGE: espforge_runtime::I2cBusStorage = espforge_runtime::I2cBusStorage::new();

pub struct Devices {
    pub oled: core::cell::RefCell<
        espforge_drivers::Ssd1306Display<espforge_drivers::Oled128x64>,
    >,
}

impl Devices {
    pub fn new(
        registry: &mut PeripheralRegistry,
    ) -> Result<Self, espforge_runtime::DeviceError> {
        let i2c0_bus: espforge_runtime::I2cBus = espforge_runtime::I2cBus::build_in(
                &__I2C0_STORAGE,
                unsafe { registry.peripherals.I2C0.clone_unchecked() },
                unsafe { registry.peripherals.GPIO6.clone_unchecked() },
                unsafe { registry.peripherals.GPIO7.clone_unchecked() },
                esp_hal::time::Rate::from_khz(400),
            )
            .expect("I2C bus configuration failed");
        Ok(Self {
            oled: core::cell::RefCell::new(
                espforge_drivers::Ssd1306Display::new(
                        espforge_runtime::embedded_hal_bus::i2c::RefCellDevice::new(
                            i2c0_bus.cell(),
                        ),
                        0x3C,
                        espforge_drivers::Oled128x64,
                        espforge_drivers::DisplayRotation::Rotate0,
                    )
                    .map_err(|kind| espforge_runtime::DeviceError {
                        device: "oled",
                        kind,
                    })?,
            ),
        })
    }
}
```

Two things to notice:

1. The endpoint is `embedded-hal-bus`'s `RefCellDevice` over the bus's shared
   cell; the address is an argument to the driver, not a runtime override.
2. The field is a `RefCell` because `Context` is shared with spawned tasks. A
   blocking project emits the bare field instead.

The `Services` struct builds the station into a local before constructing the
struct, so a client (`<ntp>`, `<http_client>`) can call `home.stack()` without
borrowing a moved value.

---

## 3. `src/lib.rs`

```rust
#[macro_export]
macro_rules! device {
    ($ctx:expr, $name:ident) => {
        $ctx.devices.$name.borrow_mut()
    };
}
```

On a blocking board the same macro expands to `&mut $ctx.devices.$name`; the
app source is the same.

The borrow rule follows from the type: a `RefMut` is `!Send`, so a future that
holds one across an `.await` cannot be spawned. That is a compile error rather
than a deadlock, and the example keeps the borrow inside one synchronous
function.

---

## 4. `src/app.rs`

```rust
fn status(ctx: &'static Context, line: &str) {
    let mut oled = device!(ctx, oled);
    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    oled.clear();
    Text::new(line, Point::new(0, 10), style).draw(&mut *oled).ok();
    oled.flush().ok();
}

pub async fn setup(ctx: &'static Context, _spawner: Spawner) {
    status(ctx, "connecting");
    service!(ctx, home).wait_connected().await;
    status(ctx, "online");
}
```

`status` borrows the device, draws and drops the guard before returning. `setup`
can then await the station without holding a non-`Send` value.

---

## 5. Boot and build

- `main` builds `Devices`, logs any `DeviceError` and halts in a one-second
  delay loop; the display's bus is configured before `esp_rtos::start`.
- `espforge gen --example ssd1306_wifi` writes the project; `cargo build
  --target riscv32imc-unknown-none-elf` compiles it.
- The Wokwi diagram is the `ssd1306` example's OLED wiring, so the serial
  monitor shows both the boot log and `Connected` after the station joins.

---

## Phase 6 update: the panel is async now

`ssd1306` gained a dual-mode recipe in phase 6. On the embassy project above
its bus now resolves **async**, so the emission in section 2 is no longer what
`espforge gen` writes: the project builds an `AsyncI2cBus`, stores the panel as
`AsyncMutex<AsyncDeferred<OledFactory>>`, and the app awaits
`async_device!(ctx, oled)` and `init().await` before drawing. The blocking
shape above still applies to the same spec on a blocking project. See
[journey-async-display.md](journey-async-display.md).

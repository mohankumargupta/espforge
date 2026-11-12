# The Journey of `spi_multi`: Two Parts, Two Configs, One Controller

This is the phase-6 walkthrough for the SPI device tier: an ILI9341 panel and
an ADXL345 accelerometer share `SPI2` while asking for different clocks and
modes.

---

## 1. The Spec

```xml
# espforge
  <project name="spi_multi" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <spi id="spi2" sclk="8" mosi="7" miso="6" mode="0" frequency="40000kHz" />
  <gpio id="gpio4" pin="4" direction="output" />
  <gpio id="gpio5" pin="5" direction="output" />
  <gpio id="gpio3" pin="3" direction="output" />

# devices
  <ili9341 id="tft" bus="$spi2" cs="9" dc="$gpio4" reset="$gpio5"
           backlight="$gpio3" rotation="90" />
  <adxl345 id="accel" bus="$spi2" cs="10" mode="3" frequency="5000kHz" />

# services
```

The bus is declared at the panel's speed, but that is only its initial
configuration. Both device recipes declare `spi_config`, so each part owns a
mode and frequency, and lowering refuses a device config the bus cannot carry
(more than 80MHz, or a mode outside `0..=3`).

---

## 2. `src/generated.rs`

One `SpiBusStorage`, one bus, two device endpoints:

```rust
let spi2_bus: espforge_runtime::SpiBus = espforge_runtime::SpiBus::build_in(
        &__SPI2_STORAGE,
        unsafe { registry.peripherals.SPI2.clone_unchecked() },
        unsafe { registry.peripherals.GPIO8.clone_unchecked() },
        unsafe { registry.peripherals.GPIO7.clone_unchecked() },
        unsafe { registry.peripherals.GPIO6.clone_unchecked() },
        espforge_runtime::SpiConfig {
            frequency: esp_hal::time::Rate::from_khz(40000),
            mode: esp_hal::spi::Mode::_0,
        },
    )
    .expect("SPI bus configuration failed");

Ok(Self {
    tft: espforge_drivers::Ili9341Display::new(
            espforge_runtime::ConfiguredSpiDevice::new(
                spi2_bus,
                esp_hal::gpio::Output::new(
                    unsafe { registry.peripherals.GPIO9.clone_unchecked() },
                    esp_hal::gpio::Level::High,
                    esp_hal::gpio::OutputConfig::default(),
                ),
                espforge_runtime::Delay::new(),
                espforge_runtime::SpiConfig {
                    frequency: esp_hal::time::Rate::from_khz(40000),
                    mode: esp_hal::spi::Mode::_0,
                },
                espforge_runtime::CsPolarity::ActiveLow,
            ),
            /* dc, reset, backlight, rotation, colour order … */
        )?,
    accel: espforge_drivers::Adxl345::new(
            espforge_runtime::ConfiguredSpiDevice::new(
                spi2_bus,
                esp_hal::gpio::Output::new(
                    unsafe { registry.peripherals.GPIO10.clone_unchecked() },
                    esp_hal::gpio::Level::High,
                    esp_hal::gpio::OutputConfig::default(),
                ),
                espforge_runtime::Delay::new(),
                espforge_runtime::SpiConfig {
                    frequency: esp_hal::time::Rate::from_khz(5000),
                    mode: esp_hal::spi::Mode::_3,
                },
                espforge_runtime::CsPolarity::ActiveLow,
            ),
        )?,
})
```

Things to notice:

1. `ConfiguredSpiDevice` replaces `embedded-hal-bus`'s `RefCellDevice` for
   devices. It owns the CS line, so it can assert it **after** applying the
   part's `SpiConfig` and release it in the opposite direction.
2. The bus remembers the last configuration as a packed `(kHz, mode)` atomic.
   A transaction whose config matches does no extra bus work; alternating
   parts do one `apply_config` each.
3. `CsPolarity::ActiveLow` is the default. A local driver recipe can say
   `cs_polarity "high"` and get the same adapter driving an active-high
   select; no bundled part needs it.
4. Both parts are built inside `Devices::new`, after the one bus. The `?:`
   maps each driver failure to a `DeviceError` naming `tft` or `accel`.

---

## 3. `src/app.rs`

```rust
pub fn setup(ctx: &mut Context) {
    let tft = device!(ctx, tft);
    Rectangle::new(Point::zero(), Size::new(320, 240))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0, 0, 20)))
        .draw(tft)
        .ok();

    match device!(ctx, accel).read() {
        Ok(reading) => ctx.logger.info(format_args!("ADXL345 {:?} mg", reading.milli_g())),
        Err(error) => ctx.logger.warn(format_args!("ADXL345 read failed: {error:?}")),
    }
}
```

`forever` repeats the read every 200ms. The two `device!` borrows never
overlap: the panel borrow ends before the sensor's begins.

---

## 4. Boot and build

- `main` builds the bus and both devices, logging a named `DeviceError` if
  either part does not answer.
- `espforge gen --example 04.Communication/spi_multi` writes the project;
  `cargo build --target riscv32imc-unknown-none-elf` compiles it.
- The Wokwi diagram wires the shared SCLK/MOSI/MISO, CS9 for the panel, CS10
  for the sensor, and the panel's DC/reset/backlight pins.

---

## 7. The async counterpart

`04.Communication.spi_async` is the async end of the same tier: one ADXL345 on
an `AsyncSpiBus`, with `ConfiguredAsyncSpiDevice` applying its own mode and
clock before CS. This example remains blocking on purpose — it is the proof
that per-device configuration works on the blocking path, and its goldens are
byte-identical in 0.8.0.

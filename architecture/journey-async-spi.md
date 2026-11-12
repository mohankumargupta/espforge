# Journey: an async SPI device

`04.Communication.spi_async` puts one ADXL345 accelerometer on an embassy
board. The bus resolves to the async controller, the device endpoint applies
the part's own mode and clock before chip select, and the driver is built after
boot because its bus is async.

It is deliberately the smallest SPI example: `spi_multi` remains the proof that
two blocking parts with different settings share one controller, and this one
is the proof that the async device tier exists at all.

---

## 1. The spec

```espf
# espforge
  <project name="spi_async" />
  <chip type="esp32c3" />
  <runtime type="embassy" />

# peripherals
  <spi id="spi2" sclk="8" mosi="7" miso="6" mode="3" frequency="5000kHz" />
  <gpio id="gpio10" pin="10" direction="output" />

# devices
  <adxl345 id="accel" bus="$spi2" cs="$gpio10" mode="3" frequency="5000kHz" init="deferred" />

# services
```

`adxl345` declares both bus modes:

```kdl
runtime "blocking"
runtime "async" {
    feature "adxl345-async"
    type "espforge_drivers::AsyncAdxl345"
    new "espforge_drivers::AsyncAdxl345::new"
    endpoint "espforge_drivers::AsyncAdxl345Endpoint"
}
```

Bus mode is resolved from the drivers bound to the bus, not from the scheduler
alone: an embassy board with a blocking-only driver keeps a blocking bus. Here
the driver is dual-mode and the project is embassy, so the bus is
`BusMode::Async`. An async-only driver beside a blocking-only one on one bus is
still refused at lowering (`E0909`).

---

## 2. The generated bus and endpoint

```rust
let spi2_bus: espforge_runtime::AsyncSpiBus = espforge_runtime::AsyncSpiBus::build_in(
    &__SPI2_STORAGE,
    peripherals.SPI2,
    peripherals.GPIO8,
    peripherals.GPIO7,
    peripherals.GPIO6,
    espforge_runtime::SpiConfig {
        frequency: esp_hal::time::Rate::from_khz(5000),
        mode: esp_hal::spi::Mode::_3,
    },
)
.expect("SPI bus configuration failed");
```

and, for the device:

```rust
espforge_runtime::ConfiguredAsyncSpiDevice::new(
    spi2_bus,
    esp_hal::gpio::Output::new(
        peripherals.GPIO10,
        esp_hal::gpio::Level::High,
        esp_hal::gpio::OutputConfig::default(),
    ),
    espforge_runtime::Delay::new(),
    espforge_runtime::SpiConfig {
        frequency: esp_hal::time::Rate::from_khz(5000),
        mode: esp_hal::spi::Mode::_3,
    },
    espforge_runtime::CsPolarity::ActiveLow,
)
```

`ConfiguredAsyncSpiDevice` is the async mirror of `ConfiguredSpiDevice`: each
transaction applies the part's configuration, asserts CS across every `.await`,
and releases it afterwards. The CS line is a field the adapter owns, so no
other task can release it mid-transaction; the bus is shared through
`AsyncSpiBus`'s mutex.

No blocking `SpiBus` is built for this project. The grep that pins it:

```sh
rg 'AsyncSpiBus::build_in|ConfiguredAsyncSpiDevice::new' src/generated.rs
rg 'SpiBus::build_in' src/generated.rs      # absent
```

---

## 3. Deferred init

`Devices::new` is synchronous, so it cannot await the driver's identity check.
The generated wiring stores a factory:

```rust
pub struct AccelFactory {
    endpoint: espforge_drivers::AsyncAdxl345Endpoint,
}

impl espforge_runtime::AsyncDeferredInit for AccelFactory {
    type Device = espforge_drivers::AsyncAdxl345;

    fn init(self) -> impl Future<Output = Result<Self::Device, DeviceError>> {
        async move {
            espforge_drivers::AsyncAdxl345::new(self.endpoint)
                .await
                .map_err(|kind| DeviceError { device: "accel", kind })
        }
    }
}
```

and the field is `AsyncMutex<AsyncDeferred<AccelFactory>>`. The factory future
is deliberately not `Send`: esp-hal handles are not, and the app awaits `init`
from `setup`, never from a spawned task.

---

## 4. The app

```rust
async fn read_once(ctx: &'static Context) {
    let mut slot = async_device!(ctx, accel);
    let accel = match slot.init().await {
        Ok(accel) => accel,
        Err(error) => {
            ctx.logger.warn(format_args!("ADXL345 init failed: {error}"));
            return;
        }
    };

    match accel.read().await {
        Ok(reading) => {
            let (x, y, z) = reading.milli_g();
            ctx.logger.info(format_args!("ADXL345 {x} {y} {z} mg"));
        }
        Err(error) => ctx.logger.warn(format_args!("ADXL345 read failed: {error:?}")),
    }
}
```

`async_device!` hands out a mutex guard rather than a `RefCell` borrow, so
holding it across `init().await` and `read().await` is sound. `forever` waits
200ms and repeats.

---

## 5. Why `spi_multi` stays blocking

`spi_multi` is the per-device-config acceptance test: two parts with different
mode and clock on one blocking controller. Changing it to embassy would blur
two proofs — per-device configuration and the async device tier — and would
stop exercising the blocking `ConfiguredSpiDevice`. Its goldens are unchanged
in 0.8.0, which is the regression proof that the async work did not touch the
blocking path.

---

## 6. Build

```sh
espforge gen --example 04.Communication/spi_async --out target/spi_async
cd target/spi_async && cargo build --target riscv32imc-unknown-none-elf
```

The Wokwi diagram wires SCLK 8, MOSI 7, MISO 6 and CS 10, exactly as the
blocking example's sensor half does.

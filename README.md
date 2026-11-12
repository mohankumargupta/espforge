# Espforge

A scaffolding++ tool: start with a `.espf` file and a simple `src/app.rs`, and it
generates an esp32 `no_std` esp-hal based rust project with the boilerplate taken
care of for you.

## Features
- instead of esphome style yaml, use a custom format that combines markdown headers
  with xml self-closing tags (see espf file format https://github.com/mohankumargupta/espf)
- Pre-built components and devices that sit on top of esphal https://github.com/esp-rs/esp-hal
- Supports both bare-metal blocking execution and async execution using the embassy framework.
- Wokwi integration and working examples

## Essence of espforge - blink example

Given blink.espf

```xml
====================
BLINK EXAMPLE
====================

# espforge
  <project name="blink" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals

# devices
  <led id="red_led" pin="18" />

# services
```

and this src/app.rs

```rust
use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("blink example");
}

pub fn forever(ctx: &mut Context) {
    let led = device!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000);
}
```

when you run

```sh
espforge build blink.espf
```

then you should have an esp32 no_std rust project ready to upload. `espforge build` writes the
project to `build/` (change it with `--out`) and compiles it.

If you want to see the examples in this repo in action, click [here](https://github.com/mohankumargupta/espforge_wokwi)

If you want to see what is the generated rust no_std examples look like,
click [here](https://github.com/mohankumargupta/espforge-extras/tree/main/espforge-examples-generated)

## Drivers(advanced)

Bundled drivers live in `espforge-drivers/manifests/<tag>.kdl`; a project can
add its own under `drivers/`, or fetch one and keep it in the cache:

```sh
espforge driver add drivers/my_sensor.kdl
espforge driver add https://example.com/my_sensor-0.1.0.tar.gz
espforge driver add git+https://example.com/my_sensor#<commit>
espforge driver add my_sensor@0.1.0 --registry https://example.com/index.json
espforge driver list
espforge driver verify
espforge driver migrate old_driver.kdl --write
```

Generation is offline. A fetched driver is cached under
`$XDG_CACHE_HOME/espforge/drivers/<tag>/<version>/` and recorded in
`.espforge/drivers.lock` with its manifest and payload hashes; only `driver
add`, `driver update` and `driver verify --refresh` reach the network. See
[`architecture/journey-driver-manifest-v2.md`](architecture/journey-driver-manifest-v2.md)
for the manifest grammar.

## Prerequisites
**Rust**: [Install Rust](https://rustup.rs/)

## Install espforge

```shell
cargo install cargo-binstall
cargo binstall espforge
# alternatively: cargo install espforge
```

## Verify install

```shell
espforge
espforge --version
```

## Run espforge doctor

```
espforge doctor
```

Depending on the targets you want, you will either install
additional toolchain into standard rust, or if you want xtensa based
ones you will be asked to use espup install.

## Walkthrough

![espforge new](https://cdn.jsdelivr.net/gh/mohankumargupta/assets@refs/heads/main/espforge-new-clone.svg)

## Examples

`espforge examples` lists the embedded examples. `espforge new` copies one into a
directory as an editable project:

```sh
espforge examples

# asks for the category, the example, the chip and the directory:
espforge new

# or answer all three up front and it never prompts:
espforge new --example blink --chip esp32c3 --dir blink

cd blink
espforge build
```

For scripted generation, use `espforge gen --example <example> --out <dir>`. The
table below is what `espforge examples` lists:

| Category | Examples |
|---|---|
| `01.Basics` | `blink`, `blink_async`, `helloworld`, `minimal` |
| `02.Digital` | `button`, `button_async` |
| `04.Communication` | `i2c_example`, `i2c_example_async`, `i2c_multi`, `i2c_scanner`, `spi_async`, `spi_example`, `spi_example_async`, `spi_multi`, `uart_example`, `uart_example_async` |
| `05.Networking` | `http_client`, `https_client`, `ntp`, `wifi_station`, `ws_client`, `wss_client` |
| `06.Displays` | `ili9341`, `ssd1306`, `ssd1306_async`, `ssd1306_wifi` |


## License

MIT

## projects for inspiration

-  espresif iot solution https://github.com/espressif/esp-iot-solution
-  esphome
-  zephyr
-  circuitpython

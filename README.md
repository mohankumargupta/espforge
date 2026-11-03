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
  <gpio id="gpio18" pin="18" direction="output" />

# components
  <led id="red_led" gpio="$gpio18" />

# devices

# services
```

and this src/app.rs

```rust
use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("blink example");
}

pub fn forever(ctx: &mut Context) {
    let led = component!(ctx, red_led);
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

## Examples

`espforge examples` lists the embedded examples. `espforge new` copies one into a
directory as an editable project:

```sh
espforge examples
espforge new 01.Basics/blink my-app
cd my-app
espforge build
```

A short name such as `blink` works too when it is unique. The table below is what
`espforge examples` lists:

| Category | Examples |
|---|---|
| `01.Basics` | `blink`, `blink_async`, `helloworld`, `minimal` |
| `02.Digital` | `button`, `button_async` |
| `04.Communication` | `i2c_example`, `i2c_example_async`, `i2c_scanner`, `spi_example`, `spi_example_async`, `uart_example`, `uart_example_async` |
| `05.Networking` | `http_client`, `https_client`, `ntp`, `wifi_station`, `ws_client`, `wss_client` |
| `06.Displays` | `ili9341`, `ssd1306` |


## License

MIT

## Similar projects for inspiration

-  espresif iot solution https://github.com/espressif/esp-iot-solution
-  esphome
-  zephyr
-  circuitpython

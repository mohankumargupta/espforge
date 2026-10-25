# Espforge

A scaffolding++ tool, stsrt with a .espf and simple src/app.rs,
generates a esp32 no_std esp-hal based rust project with boilerplate
taken care for you.

## Features
- instead of esphome style yaml, use a custom format that combines markdown headers
  with xml self-closing tags (see espf file format https://github.com/mohankumargupta/espf)
- Pre-built components and devices that sit on top of esphal https://github.com/esp-rs/esp-hal
- Supports both bare-metal blocking execution and async execution using the embassy framework.
- Wokwi integration and working examples
- code generated for examples in espforge-examples-generated for your perusal(coming soon;
- Project samples (coming soon)

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
espforge check blink.espf
espforge build blink.espf
cd build
cargo build
```

then you should have a blinking led on gpio 18. 

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
espforge doctor
```

It will recommend toolchains to install such as 

```shell
# c2,c3
rustup target add riscv32imc-unknown-none-elf
# c6,h2
rustup target add riscv32imac-unknown-none-elf riscv32imafc-unknown-none-elf
# p4
rustup target add riscv32imafc-unknown-none-elf
```

## Examples

Run `espforge examples` to use as starting point, from interactive to noninteractive versions

```sh
espforge examples categories
espforge examples list 01.Basics
espforge examples get 01.Basics/blink --out blink
```

| Category | Examples |
|---|---|
| `01.Basics` | `helloworld`, `blink` |
| `02.Digital` | `button`, `button_async` |
| `04.Communication` | `i2c_example`, `i2c_example_async`, `i2c_scanner`, `spi_example`, `spi_example_async`, `uart_example`, `uart_example_async` |
| `05.Networking` | `wifi_station` |
| `06.Displays` | `ili9341_example`, `ssd1306_example` |


## License

MIT

## Similar projects for inspiration

-  espresif iot solution https://github.com/espressif/esp-iot-solution
-  esphome
-  zephyr
-  circuitpython


# Espforge

A scaffolding++ tool for esp32 no_std rust projects.

## Features
- Uses esphome-like YAML configuration
- Pre-built components and devices that sit on top of esphal https://github.com/esp-rs/esp-hal
- Wire "app" code in rust. 
- Supports both bare-metal blocking execution and async execution using the embassy framework.
- Wokwi integration and working examples
- Project samples

## Essence of espforge - blink example

Given this yaml file

```yaml
espforge:
  name: blink
  platform: esp32c3

esp32:
  gpio:
    gpio2: { pin: 18, direction: output }

components:
  red_led:
    using: LED
    with:
      gpio: $gpio2
      active_low: false
```

and this app.rs

```rust
#[allow(unused_variables)]

use crate::{component, Context};

pub fn setup(ctx: &mut Context) {
    let logger = ctx.logger;

    logger.info("Starting Blink Example");
}

pub fn forever(ctx: &mut Context) {
    let delay = ctx.delay;
    let led = component!(red_led);

    led.toggle();
    delay.delay_ms(1000);
}
```

It will generate a esp32 esphal-based rust project that you can then

```sh
cargo build
```

If you want to see the examples in this repo in action, click [here](https://github.com/mohankumargupta/espforge_wokwi)

## Prerequisites
**Rust**: [Install Rust](https://rustup.rs/)

**ESP machinery(can always use cargo install if needed)**:
   ```shell
   cargo install cargo-binstall
   cargo binstall espup
   espup install
   cargo binstall esp-generate
  ```
  

## Installation
```shell
cargo binstall espforge
```

Alternatively

```shell
cargo install espforge
```

## Verify install

```shell
espforge doctor
```

## Geting started

![](https://cdn.jsdelivr.net/gh/mohankumargupta/assets@refs/heads/main/espforge.webp)

Run

```shell
espforge examples
```

Pick a category eg 01.Basics, then pick an example, eg. blink

This will create a generated folder with artifacts that include a blink.yaml

In that folder, run

```shell
espforge compile blink.yaml
```

This creates a rust project in-place alongside the artifacts created in the previous step.

Finally:

```shell
cargo build
```



## Updating yaml file

When the yaml file is updated, simply run:

```shell
espforge compile blink.yaml
cargo build
```

## Custom dependencies

If your project requires additional Rust crates, you can create a **dependencies.toml** file in the same directory as your project's YAML configuration. When you run espforge compile, the dependencies specified in this file will be automatically merged into the generated Cargo.toml.

Example dependencies.toml:
```toml 
[dependencies] 
my-custom-crate = "1.0.0" 
another-crate = { version = "0.2, default-features = false } 
```

## Building from source

```shell
cargo build -p espforge
```

## Wokwi 

If using VSCode, enable wokwi extension, then double-click on diagram.json

## Projects
in the **espforge_projects** folder

1. using mousefood/ratatui with ili9341 to create a menu


## add more devices

environment variable `ESPFORGE_LOCAL_PATH` can be set to a local copy of espforge repo, there you can add platforms, components and devices.

## License

MIT


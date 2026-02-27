# Espforge

A scaffolding++ tool for esp32 no_std rust projects.

## Features
- Uses esphome-like YAML configuration
- Pre-built components and devices that sit on top of esphal https://github.com/esp-rs/esp-hal
- Wire "app" code in rust. 
- Wokwi integration and working examples
- Project samples

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

ALternatively

```shell
cargo install espforge
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

## Building from source

```shell
cargo build -p espforge
```

## Wokwi 

If using VSCode, enable wokwi extension, then double-click on diagram.json

## Projects
in the **espforge_projects** folder

1. using mousefood/ratatui with ili9341 to create a menu

## License

MIT


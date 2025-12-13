# Espforge

A scaffolding++ tool for esp32 no_std rust projects.

## Features
- Uses esphome-like YAML configuration
- Pre-built components and devices that sit on top of esphal https://github.com/esp-rs/esp-hal
- Wire "main" code using Ruchy scripting language https://github.com/paiml/ruchy
- Wokwi integration and working examples

## Prerequisites
**Rust**: [Install Rust](https://rustup.rs/)
**ESP machinery(It might be possible to use cargo-binstall for these.)**:
   ```shell
   cargo install espup
   espup install
   cargo install esp-generate
  ```
  

## Installation

```shell
cargo install espforge
```

## Geting started

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

This will create another generated folder, this time it is the actually rust project.

Change to this directory, then run

```shell
cargo build
```

## Wokwi 

If using VSCode, enable wokwi extension, then double-click on diagram.json

## License

MIT


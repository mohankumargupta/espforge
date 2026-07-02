## Compiling espforge

MUST use this to compile espforge.

```sh
cargo build -p espforge
```



## Espforge concepts

Distinction between peripherals, components and devices.

1. ESP32 Peripherals are specified in YAML under esp32 key.
2. Components are specified in YAML under components key.
   They are the building blocks for devices
3. Devices are specified in YAML under devices key.
   They primarily only use components, although they can refer to pins directly.







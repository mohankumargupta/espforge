
## Instructions for codex

- Never delete files
- Only provide answer in git diff format to any code changes or features requested

## Espforge concepts

Distinction between peripherals, components and devices.

1. ESP32 Peripherals are specified in YAML under esp32 key.
2. Components are specified in YAML under components key.
   They are the building blocks for devices
3. Devices are specified in YAML under devices key.
   They primarily only use components, although they can refer to pins directly.

## Similar projects to compare how they approach things

-  espresif iot solution https://github.com/espressif/esp-iot-solution
-  esphome
-  zephyr
-  circuitpython






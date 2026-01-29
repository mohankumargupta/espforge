# Espforge Architecture

## Principle: Devices Never Access Hardware Directly

```
┌─────────────────────────────────────────────┐
│          Application Layer (app.rs)          │
│  - Uses devices.oled, devices.display        │
└─────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────┐
│            Device Layer (Devices)            │
│  - ssd1306: uses I2cDevice component         │
│  - ili9341: uses SpiDevice component         │
│  - Takes ownership of GPIO pins              │
└─────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────┐
│         Component Layer (Components)         │
│  - I2cDevice: wraps I2C peripheral           │
│  - SpiDevice: wraps SPI peripheral           │
│  - LED: wraps GPIO output                    │
│  - Button: wraps GPIO input                  │
└─────────────────────────────────────────────┘
                      ↓
┌─────────────────────────────────────────────┐
│      Hardware Layer (PeripheralRegistry)     │
│  - I2C peripherals (RefCell<I2c>)            │
│  - SPI peripherals (RefCell<Spi>)            │
│  - GPIO pins (RefCell<Option<AnyPin>>)       │
└─────────────────────────────────────────────┘
```

## Example: SSD1306 OLED Display

```yaml
esp32:
  i2c:
    i2c0: { i2c: 0, sda: 6, scl: 5, frequency_khz: 100 }

components:
  i2c_master:
    using: I2cDevice
    with:
      i2c: $i2c0        # ← Component wraps peripheral

devices:
  oled:
    using: ssd1306
    with:
      component: $i2c_master  # ← Device uses component
      address: 0x3C
```

**Flow:**
```
Hardware: registry.i2c0 (I2C peripheral)
    ↓
Component: components.i2c_master (I2cDevice wrapper)
    ↓
Device: devices.oled (SSD1306 display)
```

## Example: ILI9341 TFT Display

```yaml
esp32:
  spi:
    spi2: { spi: 2, sck: 3, mosi: 4, frequency_kHz: 10000 }
  gpio:
    pin_dc:  { pin: 6, direction: output }
    pin_rst: { pin: 7, direction: output }
    pin_cs:  { pin: 5, direction: output }

components:
  main_spi:
    using: SpiDevice
    with:
      spi: $spi2        # ← Component wraps peripheral

devices:
  display:
    using: ili9341
    with:
      spi: $main_spi   # ← Device uses component for bus
      dc: $pin_dc      # ← Device owns control pins
      rst: $pin_rst
      cs: $pin_cs
```

**Flow:**
```
Hardware: registry.spi2 (SPI peripheral)
    ↓
Component: components.main_spi (SpiDevice wrapper)
    ↓                              ↓
Device: devices.display ← registry.pin_dc/rst/cs (GPIO pins)
(ILI9341 TFT)           ← takes ownership
```
###  **Bus Sharing**
Multiple devices can share a bus via components:

```yaml
components:
  i2c_master:
    using: I2cDevice
    with:
      i2c: $i2c0

devices:
  oled:
    using: ssd1306
    with:
      component: $i2c_master
      address: 0x3C
  
  sensor:
    using: bme280
    with:
      component: $i2c_master  # Same component, different address
      address: 0x76
```


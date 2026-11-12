### The Rule for `# peripherals`

> **Rule of Thumb:**  
> **If it is fabricated onto the ESP32 silicon die (has hardware registers, interrupt lines, DMA channels, or clock dividers), it belongs in `# peripherals`.**  
> If you have to solder it, plug it in, or buy it as an external chip/component, it belongs in `# devices`.

In this revised architecture, `# peripherals` is where you configure **on-chip hardware controllers and buses**—especially those that multiple devices share, or that require hardware-level clock, buffer, and timing configuration.

---

### 10 Concrete Examples of `# peripherals`

#### 1. Shared I2C Bus Controller (`<i2c>`)
* **What it configures on silicon:** Which hardware instance (`I2C0` vs `I2C1`), physical SDA/SCL pad routing via the GPIO matrix, bus clock speed (`100kHz`, `400kHz`, `1MHz`), and bus timeout cycles.
* **Why it needs its own peripheral declaration:** You don't want to re-specify SDA/SCL and 400kHz on 4 different I2C sensors. The bus is configured once on the MCU; all devices attach to it.
```xml
# peripherals
  <i2c id="i2c0" sda="6" scl="7" frequency="400kHz" timeout="20ms" />

# devices
  <ssd1306 id="display" bus="$i2c0" address="0x3C" />
  <scd4x   id="sensor"  bus="$i2c0" />
```

---

#### 2. SPI Bus with DMA (`<spi>`)
* **What it configures on silicon:** The SPI peripheral (`SPI2`/FSPI or `SPI3`/HSPI), SCK, MOSI, MISO pins, max clock rate, and hardware DMA channel allocation (e.g., `dma="channel1"` or `dma="auto"`).
* **Why it needs its own peripheral declaration:** High-speed SPI peripherals share a DMA channel and data lines between displays, SD cards, and external flash, while each device has its own Chip Select (CS) pin.
```xml
# peripherals
  <spi id="spi2" sclk="8" mosi="7" miso="6" dma="auto" />

# devices
  <ili9341 id="screen" bus="$spi2" cs="9" dc="4" />
  <sd_card id="storage" bus="$spi2" cs="10" />
```

---

#### 3. Hardware UART / Serial Port (`<uart>`)
* **What it configures on silicon:** Hardware UART controller (`UART0`, `UART1`, `UART2`), baud rate, parity, stop bits, hardware flow control lines (RTS/CTS), and hardware RX FIFO threshold interrupts.
* **Why it needs its own peripheral declaration:** Modems, GPS modules, or RS-485 transceivers require strict hardware FIFO and baud generation independent of application logic.
```xml
# peripherals
  <uart id="uart1" tx="17" rx="16" baud_rate="115200" rts="19" cts="18" />

# devices
  <sim7000 id="lte_modem" uart="$uart1" power_pin="4" />
```

---

#### 4. Wi-Fi 2.4GHz / 5GHz Radio (`<wifi>`)
* **What it configures on silicon:** The internal RF modem, RF calibration data block, baseband power management, antenna select (if using RF switch pins), and country code.
* **Why it needs its own peripheral declaration:** The radio is a single physical block on chip. Both station mode (connecting to Wi-Fi) and access point mode share this single radio peripheral.
```xml
# peripherals
  <wifi id="radio" tx_power="20dBm" country="AU" />

# devices / services
  <wifi_station id="home_net" wifi="$radio" ssid="MyHome" />
```

---

#### 5. BLE / Bluetooth Controller (`<bluetooth>`)
* **What it configures on silicon:** The Bluetooth Link Controller, coexistence arbitration matrix with the Wi-Fi radio (to avoid RF collisions), and HCI buffer allocation.
* **Why it needs its own peripheral declaration:** In dual-mode (Wi-Fi + BLE) firmware, the Bluetooth peripheral must negotiate radio time-slots with the Wi-Fi peripheral.
```xml
# peripherals
  <bluetooth id="bt0" mode="ble" coexistence="true" />
```

---

#### 6. ADC Unit with Calibration (`<adc>`)
* **What it configures on silicon:** Analog-to-Digital Converter unit (`ADC1` vs `ADC2`), hardware attenuation (e.g. `11dB` to read up to 3.3V), resolution (12-bit), and eFuse calibration curves.
* **Why it needs its own peripheral declaration:** On the ESP32, `ADC2` cannot be used when Wi-Fi is transmitting! Configuring the ADC peripheral on the chip level allows the compiler to validate Wi-Fi/ADC pin conflicts before you compile.
```xml
# peripherals
  <adc id="adc1" unit="1" attenuation="11dB" resolution="12bit" />

# devices
  <potentiometer id="dial" adc="$adc1" pin="2" />
  <battery_monitor id="batt" adc="$adc1" pin="3" divider_ratio="2.0" />
```

---

#### 7. LEDC / Hardware PWM Timer (`<ledc>` or `<pwm>`)
* **What it configures on silicon:** High-speed or low-speed PWM timer frequency (e.g. `5000Hz` for LEDs, `50Hz` for servos), clock source (`APB_CLK`), and bit depth (e.g., 14-bit resolution).
* **Why it needs its own peripheral declaration:** Multiple PWM channels share a single hardware timer base. If you change the frequency of the timer, all pins attached to that timer scale together.
```xml
# peripherals
  <ledc id="pwm_timer0" frequency="50Hz" resolution="14bit" />

# devices
  <servo id="claw" timer="$pwm_timer0" pin="18" />
  <servo id="arm"  timer="$pwm_timer0" pin="19" />
```

---

#### 8. RMT (Remote Control / Precision Pulse Engine) (`<rmt>`)
* **What it configures on silicon:** The ESP32's dedicated microsecond pulse generator/receiver channels, carrier frequency, clock divider, and memory block size.
* **Why it needs its own peripheral declaration:** Addressable LEDs (WS2812B/NeoPixels) and Infrared transmitters require nanosecond pulse accuracy that normal GPIOs cannot do without jitter.
```xml
# peripherals
  <rmt id="rmt0" channel="0" clock_divider="4" />

# devices
  <ws2812 id="rgb_strip" rmt="$rmt0" pin="8" leds="64" />
```

---

#### 9. TWAI / CAN Bus Controller (`<twai>` / `<can>`)
* **What it configures on silicon:** The Two-Wire Automotive Interface (CAN 2.0B / ISO 11898-1 controller), TX/RX pins, baud rate (`250kbps`, `500kbps`, `1Mbps`), and hardware acceptance filter masks.
* **Why it needs its own peripheral declaration:** The CAN controller filters packets in silicon hardware registers before waking the CPU.
```xml
# peripherals
  <twai id="can0" tx="4" rx="5" baud_rate="500kbps" mode="normal" />

# devices
  <obd2_reader id="car_telemetry" bus="$can0" />
```

---

#### 10. Hardware Watchdog / RTC Timer (`<wdt>` / `<timer>`)
* **What it configures on silicon:** Hardware Task Watchdog Timer (TWDT) or Real-Time Counter timer group (`TIMG0`, `TIMG1`), auto-reload, and hardware reboot timeout.
* **Why it needs its own peripheral declaration:** System reliability controllers must be independently configured so the silicon resets the board if user firmware deadlocks.
```xml
# peripherals
  <wdt id="system_wdt" timeout="5000ms" panic_on_trigger="true" />
  <timer id="timer0" group="0" timer="0" prescaler="80" />
```

---

### Comparison Checklist

| Hardware Entity | Section | Why |
|---|---|---|
| I2C0, I2C1 | `# peripherals` | Silicon controller; manages bus arbitration, clock, SDA/SCL lines |
| SSD1306 Display | `# devices` | External glass panel with a controller; plugs into I2C |
| UART1 | `# peripherals` | Silicon hardware engine; generates serial baud rate and buffers |
| GPS Module | `# devices` | External board; sends NMEA serial text over UART |
| ADC1 | `# peripherals` | Silicon analog-to-digital converter, reference voltage, eFuse cal |
| Soil Moisture Sensor | `# devices` | External probe connected to an ADC pin |
| PWM Timer | `# peripherals` | Silicon clock counter setting waveform frequency and resolution |
| Servo Motor | `# devices` | External physical motor driven by a PWM channel |
| Wi-Fi Radio | `# peripherals` | Silicon 2.4GHz RF transceiver and MAC engine |
| Push Button / LED | `# devices` | Passive external components; they don't own silicon engines |

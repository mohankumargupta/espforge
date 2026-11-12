Here are **10 more concrete examples** of on-chip silicon blocks that belong strictly in `# peripherals`. 

These cover audio, motor control, high-speed storage, low-power coprocessing, and modern mesh radios available across the ESP32 family (ESP32, S2, S3, C6, H2, and P4).

---

### 1. I2S Audio Bus Controller (`<i2s>`)
* **What it configures on silicon:** Dedicated audio clock dividers (`MCLK`, `BCLK`, `WS`), bit depth (16, 24, 32-bit), communication format (Philips standard, MSB, TDM/PCM), and DMA FIFO buffers.
* **Why it belongs in `# peripherals`:** I2S requires strict, jitter-free sample clocks derived from the chip's internal audio PLL. You can have a speaker (output) and a microphone (input) sharing the same I2S clock line.
```xml
# peripherals
  <i2s id="i2s0" standard="philips" sample_rate="44100Hz" 
       bclk="14" ws="15" dout="22" din="23" dma_buffers="4" />

# devices
  <max98357a id="amplifier" bus="$i2s0" />
  <inmp441   id="microphone" bus="$i2s0" />
```

---

### 2. PCNT (Hardware Pulse Counter) (`<pcnt>`)
* **What it configures on silicon:** Dedicated hardware counter registers with glitch filters, edge detection (count on rising/falling edge), and control pin thresholds (invert count if control pin is high/low).
* **Why it belongs in `# peripherals`:** The hardware counter counts pulses **in hardware without waking the CPU**. Multiple encoders share the 4 to 8 physical PCNT units on the chip.
```xml
# peripherals
  <pcnt id="pcnt_unit0" filter_ns="1000" min_limit="-10000" max_limit="10000" />

# devices
  <!-- A quadrature rotary encoder uses 2 PCNT channels -->
  <rotary_encoder id="volume_knob" pcnt="$pcnt_unit0" pin_a="4" pin_b="5" />
  <flow_meter     id="water_flow"  pcnt="$pcnt_unit0" pin="18" />
```

---

### 3. MCPWM (Advanced Motor Control PWM) (`<mcpwm>`)
* **What it configures on silicon:** *Distinct from simple LED PWM.* Configures dead-time generators (prevents shoot-through shorts on complementary MOSFET gates), hardware emergency fault trip inputs, and carrier frequency synchronization.
* **Why it belongs in `# peripherals`:** H-bridge motor drivers and 3-phase BLDC inverters require microsecond-accurate dead-time insertion directly inside the silicon logic.
```xml
# peripherals
  <mcpwm id="motor_pwm0" frequency="20kHz" dead_time_ns="500" fault_pin="21" />

# devices
  <h_bridge_motor id="drive_motor" controller="$motor_pwm0" high_pin="18" low_pin="19" />
```

---

### 4. Native SD/MMC Parallel Bus (`<sdmmc>`)
* **What it configures on silicon:** The chip's dedicated 1-bit / 4-bit / 8-bit high-speed SD host controller, bus clock divider (e.g. 40MHz), and command/response registers.
* **Why it belongs in `# peripherals`:** *This is not SPI mode.* The native SDMMC controller uses hardware-level 4-line parallel data transfers (`D0`, `D1`, `D2`, `D3`, `CLK`, `CMD`) with built-in CRC generation.
```xml
# peripherals
  <sdmmc id="sd_host" width="4bit" frequency="40MHz" 
         clk="14" cmd="15" d0="2" d1="4" d2="12" d3="13" />

# devices
  <microsd_card id="sd_storage" host="$sd_host" />
```

---

### 5. IEEE 802.15.4 Radio (Thread / Zigbee / Matter) (`<ieee802154>`)
* **What it configures on silicon:** The on-chip 2.4 GHz 802.15.4 PHY/MAC modem (found on ESP32-C6 and ESP32-H2), radio channel (11–26), CCA (Clear Channel Assessment) threshold, and hardware frame filtering.
* **Why it belongs in `# peripherals`:** It is a physical RF transceiver on the die, separate from Wi-Fi.
```xml
# peripherals
  <ieee802154 id="mesh_radio" channel="15" tx_power="20dBm" />

# services
  <matter_node id="smart_switch" radio="$mesh_radio" />
  <zigbee_router id="zigbee_mesh" radio="$mesh_radio" />
```

---

### 6. Built-in Capacitive Touch Controller (`<touch>`)
* **What it configures on silicon:** Dedicated internal charge/discharge timing engine, sleep-wake sensitivity threshold, hardware IIR noise filter, and touch pad scan rate.
* **Why it belongs in `# peripherals`:** Touch pads are routed to specific fixed analog-capable pins and share an internal measurement sequencer.
```xml
# peripherals
  <touch id="touch_engine" filter_period="10ms" debounce_cycles="3" />

# devices
  <touch_button id="power_touch" engine="$touch_engine" pin="4" threshold="200" />
  <touch_slider id="light_slider" engine="$touch_engine" pins="12,13,14" />
```

---

### 7. Native USB-OTG / USB-Serial-JTAG (`<usb>`)
* **What it configures on silicon:** Internal USB Full-Speed PHY, dedicated `D+` and `D-` line routing, USB pull-up/pull-down resistor state, and endpoint RAM buffers.
* **Why it belongs in `# peripherals`:** Setting the USB controller to Host mode vs. Device mode fundamentally changes how the hardware PHY routes signals.
```xml
# peripherals
  <usb id="usb0" mode="device" speed="full_speed" dp="20" dm="19" />

# devices
  <!-- Emulates a computer keyboard over USB -->
  <usb_hid_keyboard id="macro_keypad" controller="$usb0" />
```

---

### 8. Hardware DAC (Digital-to-Analog Converter) (`<dac>`)
* **What it configures on silicon:** The internal 8-bit R-2R resistor ladder, voltage reference selection, and cosine waveform generator (CWG).
* **Why it belongs in `# peripherals`:** Unlike PWM (which fakes analog with pulses), the DAC outputs genuine variable DC voltage on fixed silicon pads.
```xml
# peripherals
  <dac id="dac0" resolution="8bit" />

# devices
  <analog_voltage_source id="v_bias" dac="$dac0" pin="25" voltage="1.65V" />
  <mono_piezo_buzzer     id="beeper" dac="$dac0" pin="26" />
```

---

### 9. RTC Controller & Deep Sleep Power Domain (`<rtc>` / `<lp_core>`)
* **What it configures on silicon:** RTC slow clock selection (external 32.768kHz crystal vs. internal 150kHz oscillator), brownout detector threshold, and power domain gating during deep sleep.
* **Why it belongs in `# peripherals`:** It lives in the "Always-On" (AON) silicon domain. When the main CPU turns off during deep sleep, this peripheral remains powered.
```xml
# peripherals
  <rtc id="rtc_pwr" slow_clock="external_32k" brownout_voltage="2.8V" />

# devices
  <wake_pin id="pir_motion_wake" rtc="$rtc_pwr" pin="0" level="high" />
```

---

### 10. Hardware Crypto Engine & True Random Generator (`<crypto>`)
* **What it configures on silicon:** Dedicated silicon acceleration engines for AES-256, SHA-512, RSA-3072, ECC, and the thermal noise True Random Number Generator (TRNG).
* **Why it belongs in `# peripherals`:** It is shared infrastructure used by TLS, disk encryption, and secure boot—not an external part.
```xml
# peripherals
  <crypto id="hw_security" aes_accelerator="true" trng="true" />
```

---

### The Clean Mental Checklist

When deciding whether something goes into `# peripherals` or `# devices`, ask these 3 questions:

1. **Did it come inside the ESP32 package when you bought the bare chip?**  
   * If yes $\to$ It is a candidate for `# peripherals`.
2. **Does it control hardware registers, bus clocks, physical wave generation, or radio frequencies?**  
   * If yes $\to$ `# peripherals` (e.g. `i2c`, `spi`, `uart`, `rmt`, `i2s`, `adc`, `wifi`).
3. **Is it a passive component, a physical sensor, a breakout board, or a display?**  
   * If yes $\to$ `# devices` (e.g. `led`, `button`, `ssd1306`, `mpu6050`, `servo`, `sd_card`).

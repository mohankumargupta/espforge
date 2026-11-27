There are **two completely different reasons** the RMT examples fail in Wokwi—one in the **`rmt_tx` / `rmt_tx_async`** examples (simulator limitation + blocking read loop) and one in the **`ws2812` / `ws2812_async`** examples (actual bugs in `espforge-drivers/src/ws2812.rs` + missing `diagram.json`).

---

### 1. Why `04.Communication/rmt_tx` and `rmt_tx_async` hang in Wokwi

Both examples (`espforge-examples/src/examples/04.Communication/rmt_tx/` and `rmt_tx_async/`) wire the RMT TX pin directly to the UART1 RX pin in `diagram.json`:

```json
["esp:4", "esp:5", "green", []]
```

with `<rmt_tx ... pin="4" idle_level="high" />` and `<uart id="uart1" uart="UART1" tx="6" rx="5" baud_rate="115200" />`.

1. **Wokwi’s ESP32 UART RX does not decode bit-banged/RMT GPIO waveforms**.
   In Wokwi, the ESP32-C3 RMT TX peripheral generates digital edge transitions on `esp:4`, and the ESP32 UART peripheral works at the **byte/frame level** (via `$serialMonitor` or a Wokwi custom chip’s `uartInit`/`uartWrite`)—it does **not** oversample raw GPIO voltage edges on `esp:5` into UART bytes.
   *(Notice this is the exact same reason `rmthello.md` in `peripherals/rmt-hello/` had to build the `parts/uartwire` virtual chip for `esp-emu`: `"The emulator has no wire from an RMT channel to a UART, so the virtual part in parts/uartwire is that wire"`.)*
2. **`uart.read_line(&mut line)` loops forever waiting for `\n`.**
   In `espforge-runtime/src/hw/uart.rs`, both blocking and async `UartDevice::read_line` loop unconditionally while `read_buffered` returns `0`. Because `UART1` never receives anything in Wokwi, `forever()` prints `"TEST: rmt hello boot"` and then hangs forever on the first `read_line` call.
3. **Minor boot-order issue on real hardware too (`espforge-emit/src/codegen/rust.rs`).**
   `collect()` runs `raw_bus::emit_raw_buses` (`uart1`) **before** `Device::Rmt` (`uart_tx`). So `UART1` starts listening on GPIO 5 while GPIO 4 is still unconfigured (low/floating) before `uart_tx` drives GPIO 4 `idle_level="high"`, which can inject a spurious `0x00` / framing-error byte at boot.

**How to make `rmt_tx` / `rmt_tx_async` work in Wokwi:**
* **Best Wokwi-native option:** Loop `<rmt_tx>` into `<rmt_rx>` (Wokwi supports RMT RX capturing RMT TX edges—though in `async` mode so `rx.receive()` can listen concurrently with `tx.transmit()`), or attach a `wokwi-logic-analyzer` in `diagram.json` and don’t block `forever()` on `UART1`.
* **Or keep UART1:** Add a Wokwi custom chip (`chip.json` + `chip.wasm` using `pinWatch` + `uartWrite`, similar to `parts/uartwire`, likely at `9600` baud rather than `115200` so Wokwi’s custom-chip timestamps have plenty of margin).

---

### 2. Why `06.Displays/ws2812` and `ws2812_async` fail in Wokwi (and on real hardware)

`ws2812` and `ws2812_async` fail even before Wokwi comes into play because of **bugs in `espforge-drivers/src/ws2812.rs`** plus a missing Wokwi diagram:

1. **`EndMarkerMissing` — every `write()` call errors out immediately.**
   In `espforge-drivers/src/ws2812.rs`:
   ```rust
   let mut frame = [PulseCode::end_marker(); 24];
   for pixel in pixels {
       encode(*pixel, timings, &mut frame);
       self.channel.transmit(&frame)?;
   }
   self.channel.transmit(&[latch(timings)])?;
   ```
   `encode()` overwrites **all 24 entries** (`codes[0..24]`) with non-zero `t0h/t0l` or `t1h/t1l` durations. There is **no** `PulseCode::end_marker()` at the end of `frame` (it needs 25 elements: 24 bit-pairs + `PulseCode::end_marker()`).
   In `esp-hal 1.2.2`, `Channel::transmit` validates that the slice ends with a zero-length end marker and returns `Err(Error::EndMarkerMissing)`, so `app.rs` immediately prints `"WS2812 frame failed"` without outputting anything.
2. **Splitting pixels across separate `transmit()` calls breaks WS2812 latching.**
   Calling `self.channel.transmit(&frame)` once per pixel (waiting for `wait()` / `.await` between pixels) introduces a gap between pixels. For WS2812B, any low time $\ge 50\ \mu\text{s}$ resets/latches the strip—meaning pixel 1 and pixel 2 overwrite LED 0 instead of shifting down the strip (especially in `ws2812_async` with `.await`, and in Wokwi where simulated CPU execution is capped around ~8 MHz). All pixels + the latch/end-marker need to be sent in a single `transmit()` buffer.
3. **RGB vs. GRB byte order.**
   `app.rs` passes `[[32, 0, 0], [0, 32, 0], [0, 0, 32]]` (RGB), and `encode()` transmits `pixel[0], pixel[1], pixel[2]` verbatim instead of WS2812B’s **GRB** order (`[pixel[1], pixel[0], pixel[2]]`), swapping red and green.
4. **No `diagram.json` in `06.Displays/ws2812` or `ws2812_async`.**
   Neither example includes a `diagram.json`, so `espforge` generates the bare default diagram (`esp` + `$serialMonitor` only) with no `wokwi-neopixel` / `wokwi-led-ring` connected to GPIO 8.

---

If you'd like to continue, tell me which part you want to tackle first:
1. **Fix `ws2812` / `ws2812_async` first** (`espforge-drivers/src/ws2812.rs` single-buffer GRB + `end_marker()`, plus adding `diagram.json` with `wokwi-neopixel` wired to GPIO 8), and/or
2. **Fix `rmt_tx` / `rmt_tx_async` for Wokwi** (either switching the loopback test to `<rmt_tx>` → `<rmt_rx>` in async / adding a `wokwi-logic-analyzer`, or building a Wokwi `uartwire` custom chip).

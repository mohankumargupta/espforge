This file is a merged representation of a subset of the codebase, containing specifically included files, combined into a single document by Repomix.
The content has been processed where security check has been disabled.

# File Summary

## Purpose
This file contains a packed representation of a subset of the repository's contents that is considered the most important context.
It is designed to be easily consumable by AI systems for analysis, code review,
or other automated processes.

## File Format
The content is organized as follows:
1. This summary section
2. Repository information
3. Directory structure
4. Repository files (if enabled)
5. Multiple file entries, each consisting of:
  a. A header with the file path (## File: path/to/file)
  b. The full contents of the file in a code block

## Usage Guidelines
- This file should be treated as read-only. Any changes should be made to the
  original repository files, not this packed version.
- When processing this file, use the file path to distinguish
  between different files in the repository.
- Be aware that this file may contain sensitive information. Handle it with
  the same level of security as you would the original repository.

## Notes
- Some files may have been excluded based on .gitignore rules and Repomix's configuration
- Binary files are not included in this packed representation. Please refer to the Repository Structure section for a complete list of file paths, including binary files
- Only files matching these patterns are included: peripherals
- Files matching patterns in .gitignore are excluded
- Files matching default ignore patterns are excluded
- Security check has been disabled - content may contain sensitive information
- Files are sorted by Git change count (files with more changes are at the bottom)

# Directory Structure
````
peripherals/
  rmt-hello/
    parts/
      uartwire/
        src/
          lib.rs
        Cargo.toml
    src/
      app.rs
    board.json
    README.md
    rmt-hello.espf.txt
    uartwire.wasm
````

# Files

## File: peripherals/rmt-hello/parts/uartwire/src/lib.rs
````rust
//! A wire from the chip's RMT output to its own UART receiver.
//!
//! `rmt-hello` drives GPIO 4 with a 115200 8N1 pulse train: a low start bit,
//! eight data bits least significant first and a high stop bit, nine ticks of
//! 1 µs each. The emulator has no wire between an RMT channel and a UART, so
//! this part is that wire: it reads the pin as runs of edges
//! ([`reads_edges`]), decodes the bytes, and puts them on the UART1 line the
//! firmware's port is wired to ([`serial`]). The firmware then prints what its
//! own UART received, and the console shows the text the waveform carried.
//!
//! An RMT receiver records where the line changes, so a byte's ten bit times
//! arrive as runs: `h`'s first three low bits are one 27 µs low, not three
//! 9 µs ones. The byte is decoded by sampling the timeline at each bit's
//! centre, not by reading symbols.

use esp_part::prelude::*;

/// The chip pin the RMT channel drives. The board says which, as
/// `{ pins: { RMT: 4 } }`.
const RMT: &str = "RMT";
/// The part's own line pins, as a serial part labels them: its TX is wired to
/// the GPIO the firmware's UART receives on, its RX to the one it transmits on.
const TX: &str = "TX";
const RX: &str = "RX";

/// One 8N1 bit: nine ticks of 1 µs, the framing `rmt-hello` transmits.
const BIT_NS: u64 = 9_000;
/// Start bit, eight data bits, stop bit.
const BYTE_NS: u64 = 10 * BIT_NS;

/// The edges kept before the oldest are dropped. A frame is about sixty
/// transitions, so this holds many frames' worth even if nobody reads them.
const MAX_EDGES: usize = 512;

#[derive(Default)]
struct UartWire {
    /// The chip pin the RMT channel drives; `None` where the board left it out.
    rmt: Option<u32>,
    /// The chip pins the UART line is on, from the part's end.
    tx: Option<u32>,
    rx: Option<u32>,
    /// Where the line changed, and the level it took: `(time_ns, high)`.
    edges: Vec<(u64, bool)>,
    /// Bytes already handed to the UART, so a re-decode sends no duplicates.
    sent: usize,
}

impl UartWire {
    /// The level the line held at `at`, from the last edge at or before it.
    fn level_at(&self, at: u64) -> Option<bool> {
        self.edges
            .iter()
            .rev()
            .find(|(time, _)| *time <= at)
            .map(|&(_, level)| level)
    }

    /// One 8N1 byte starting at `start`, if `now` is far enough along for all
    /// ten bit times to be known.
    fn byte_at(&self, start: u64, now: u64) -> Option<u8> {
        if self.level_at(start)? {
            return None; // the start bit is low
        }

        let mut byte = 0u8;
        for bit in 0..8u64 {
            // The centre of data bit `bit`, which sits just after the start bit.
            let at = start + (bit + 1) * BIT_NS + BIT_NS / 2;
            if at > now {
                return None;
            }
            if self.level_at(at)? {
                byte |= 1 << bit;
            }
        }

        // Sampling the stop bit also proves the byte had its full ten bits.
        let stop = start + 9 * BIT_NS + BIT_NS / 2;
        if stop > now {
            return None;
        }
        self.level_at(stop)?.then_some(byte)
    }

    /// Every complete byte the edges hold, in order.
    fn decode(&self, now: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut next = 0;
        let mut previous: Option<bool> = None;

        for &(start, level) in &self.edges {
            // A byte starts where the line falls for its start bit.
            let falls = !level && previous != Some(false);
            if falls && start >= next {
                if let Some(byte) = self.byte_at(start, now) {
                    bytes.push(byte);
                    next = start + BYTE_NS;
                }
            }
            previous = Some(level);
        }

        bytes
    }
}

impl Device for UartWire {
    const NAME: &'static str = "uartwire";
    const ON_MESSAGE: bool = true;

    fn configure(&mut self, config: &PartConfig) {
        self.rmt = config.pins.get(RMT).copied();
        self.tx = config.pins.get(TX).copied();
        self.rx = config.pins.get(RX).copied();
    }

    /// The UART line, and the RMT pin read as runs of edges.
    fn claims(&self) -> Vec<Claim> {
        let mut claims = vec![serial(self.tx, self.rx)];
        if let Some(pin) = self.rmt {
            claims.push(reads_edges(1 << pin));
        }
        claims
    }

    fn on_edges(&mut self, pin: u32, first_level: bool, widths_ns: &[u32], at_ns: u64) {
        if Some(pin) != self.rmt {
            return;
        }
        if self.edges.len() + widths_ns.len() + 1 > MAX_EDGES {
            self.edges.clear();
            self.sent = 0;
        }

        let mut time = at_ns;
        let mut level = first_level;
        self.edges.push((time, level));
        for &width in widths_ns {
            time += u64::from(width);
            level = !level;
            self.edges.push((time, level));
        }
    }

    /// Hand over bytes decoded since the last call. The last stop bit's high
    /// run has no closing edge, so this is also where a frame's final byte is
    /// recognised: by then `now_ns` is past its centre.
    fn poll(&mut self, now_ns: u64) -> Option<EmulatorMessage> {
        let bytes = self.decode(now_ns);
        if bytes.len() <= self.sent {
            self.sent = bytes.len();
            return None;
        }
        let fresh = bytes[self.sent..].to_vec();
        self.sent = bytes.len();
        Some(transmit(&fresh, now_ns))
    }

    fn state(&self) -> Option<State> {
        Some(State::new().set("bytes", self.sent))
    }
}

// A run of edges is up to 2,581 bytes (`PART_ABI.md`); the answer is small.
esp_part::export_part!(UartWire, buffer = 4096);

#[cfg(test)]
mod tests {
    use super::*;

    const PIN: u32 = 4;

    fn wired() -> UartWire {
        let mut wire = UartWire::default();
        wire.configure(&PartConfig {
            pins: [(RMT.to_string(), PIN), (TX.to_string(), 5), (RX.to_string(), 6)].into(),
            ..Default::default()
        });
        wire
    }

    /// The levels of `message` on the wire, one a bit.
    fn levels(message: &[u8]) -> Vec<bool> {
        let mut levels = Vec::new();
        for &byte in message {
            levels.push(false); // start
            for bit in 0..8 {
                levels.push(byte >> bit & 1 == 1);
            }
            levels.push(true); // stop
        }
        levels
    }

    /// The wire as the emulator reports it: the first level, then a width for
    /// every level change. The final level stays open, as a stop bit does.
    fn runs(message: &[u8]) -> (bool, Vec<u32>) {
        let levels = levels(message);
        let mut widths = Vec::new();
        let mut width = BIT_NS;
        for pair in levels.windows(2) {
            if pair[0] == pair[1] {
                width += BIT_NS;
            } else {
                widths.push(width as u32);
                width = BIT_NS;
            }
        }
        (levels[0], widths)
    }

    fn received(wire: &mut UartWire, now: u64) -> Vec<u8> {
        match wire.poll(now).and_then(|message| message.payload) {
            Some(esp_part::Payload::UartRx(rx)) => rx.data,
            None => Vec::new(),
            Some(other) => panic!("not a UartRx: {other:?}"),
        }
    }

    const MESSAGE: &[u8] = b"hello\n";

    #[test]
    fn the_hello_frame_arrives_on_the_uart() {
        let mut wire = wired();
        let (first, widths) = runs(MESSAGE);
        let end = 1_000 + widths.iter().sum::<u32>() as u64;
        wire.on_edges(PIN, first, &widths, 1_000);
        assert_eq!(
            received(&mut wire, end),
            b"hello",
            "the last byte's stop bit is still open"
        );
        assert_eq!(received(&mut wire, end + BIT_NS), b"\n");
    }

    #[test]
    fn a_frame_in_pieces_decodes_as_one() {
        let mut wire = wired();
        let (first, widths) = runs(MESSAGE);
        let end = 1_000 + widths.iter().sum::<u32>() as u64;
        let cut = 20;
        wire.on_edges(PIN, first, &widths[..cut], 1_000);
        let at = 1_000 + widths[..cut].iter().sum::<u32>() as u64;
        // The next run starts with the level the edge at `at` left behind.
        wire.on_edges(PIN, first ^ (cut % 2 == 1), &widths[cut..], at);
        assert_eq!(received(&mut wire, end + BIT_NS), MESSAGE);
    }

    #[test]
    fn it_reads_its_pin_and_the_line_it_is_wired_to() {
        assert_eq!(
            wired().claims(),
            [serial(Some(5), Some(6)), reads_edges(1 << PIN)]
        );
    }

    #[test]
    fn another_pin_and_unwired_parts_do_nothing() {
        let mut wire = wired();
        let (first, widths) = runs(MESSAGE);
        wire.on_edges(PIN + 1, first, &widths, 0);
        assert!(wire.edges.is_empty());

        let mut unwired = UartWire::default();
        let (first, widths) = runs(MESSAGE);
        unwired.on_edges(PIN, first, &widths, 0);
        assert!(received(&mut unwired, 1_000_000).is_empty());
    }

    #[test]
    fn noise_is_not_a_byte() {
        let mut wire = wired();
        // A short low then a high: too short to be a start bit and a byte.
        wire.on_edges(PIN, false, &[3_000, 2_000], 0);
        assert_eq!(received(&mut wire, 100_000), []);
    }

    #[test]
    fn two_frames_send_twice() {
        let mut wire = wired();
        let (first, widths) = runs(MESSAGE);
        let end = 1_000 + widths.iter().sum::<u32>() as u64;
        wire.on_edges(PIN, first, &widths, 1_000);
        assert_eq!(received(&mut wire, end + BIT_NS), MESSAGE);
        assert_eq!(received(&mut wire, end + 2 * BIT_NS), [], "nothing new");
        wire.on_edges(PIN, first, &widths, end + 10 * BYTE_NS);
        let end = end + 10 * BYTE_NS + widths.iter().sum::<u32>() as u64;
        assert_eq!(received(&mut wire, end + BIT_NS), MESSAGE);
    }
}
````

## File: peripherals/rmt-hello/parts/uartwire/Cargo.toml
````toml
# A virtual part: a wire from the chip's RMT output to its own UART receiver,
# compiled to a WebAssembly module the emulator loads. `just rmt-hello-part`
# builds it and copies it beside board.json; README.md says what it models.
#
# Its own workspace (the empty [workspace] table below), so it builds on its
# own and nothing here enters the firmware's dependency graph.
[workspace]

[package]
name = "uartwire"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
# A cdylib builds to the .wasm module a host loads.
crate-type = ["cdylib"]

[dependencies]
# The part SDK: the contract's types, the `Device` trait and `export_part!`.
esp-part = { path = "../../../../esp-emulator/esp-part" }

[profile.release]
# A smaller module loads faster.
opt-level = "s"
lto = true
strip = true
````

## File: peripherals/rmt-hello/src/app.rs
````rust
//! The esp-hal `rmt-hello` example, read back through a UART.
//!
//! `uart_tx` is the raw `<rmt_tx>` channel from `rmt-hello.espf.txt`: GPIO 4,
//! clock divider 80, idle high. At 80 MHz / 80 one tick is 1 µs, so a 115200
//! baud bit (8.68 µs) is 9 ticks and each byte is start bit + 8 data bits +
//! stop bit. The app builds the pulses; the channel does not know UART.
//!
//! `uart1` is the chip's own second UART, its RX on GPIO 5. The virtual wire
//! in `parts/uartwire` watches GPIO 4 and puts the bytes it decodes on that
//! line, so the port receives what the RMT channel transmitted. Printing what
//! the UART read puts the text on the console, where the waveform started.

use esp_hal::gpio::Level;
use espforge_runtime::PulseCode;

use crate::Context;

/// 80 MHz / 80 = 1 MHz (1 tick = 1 µs), 115200 baud -> 9 ticks a bit.
const BIT_TICKS: u16 = 9;

/// 6 bytes ("hello\n") * 5 PulseCodes + 1 end marker = 31 symbols.
const FRAME_LEN: usize = 31;

/// A line from the UART; "hello\n" is six bytes.
const LINE_LEN: usize = 32;

fn byte_to_pulses(byte: u8) -> [PulseCode; 5] {
    let bit = |i: u8| {
        if (byte >> i) & 1 == 1 {
            Level::High
        } else {
            Level::Low
        }
    };

    [
        PulseCode::new(Level::Low, BIT_TICKS, bit(0), BIT_TICKS), // Start + D0
        PulseCode::new(bit(1), BIT_TICKS, bit(2), BIT_TICKS),     // D1 + D2
        PulseCode::new(bit(3), BIT_TICKS, bit(4), BIT_TICKS),     // D3 + D4
        PulseCode::new(bit(5), BIT_TICKS, bit(6), BIT_TICKS),     // D5 + D6
        PulseCode::new(bit(7), BIT_TICKS, Level::High, BIT_TICKS), // D7 + Stop
    ]
}

/// The whole message as pulse codes, ending on an end marker.
fn hello_frame() -> [PulseCode; FRAME_LEN] {
    let mut data = [PulseCode::end_marker(); FRAME_LEN];

    for (i, &byte) in b"hello\n".iter().enumerate() {
        data[i * 5..(i + 1) * 5].copy_from_slice(&byte_to_pulses(byte));
    }

    data
}

pub fn setup(ctx: &mut Context) {
    ctx.logger.info("TEST: rmt hello boot");
}

pub fn forever(ctx: &mut Context) {
    match device!(ctx, uart_tx).transmit(&hello_frame()) {
        Ok(()) => ctx.logger.info("TEST: rmt hello sent"),
        Err(_) => ctx.logger.error("TEST: rmt hello failed"),
    }

    let mut line = [0u8; LINE_LEN];
    match device!(ctx, uart1).read_line(&mut line) {
        Ok(n) => {
            let text = core::str::from_utf8(&line[..n]).unwrap_or("<not utf-8>");
            ctx.logger.info(format_args!(
                "TEST: uart {:?}",
                text.trim_end_matches(['\r', '\n'])
            ));
        }
        Err(_) => ctx.logger.error("TEST: uart read failed"),
    }

    ctx.delay.delay_ms(1_000);
}
````

## File: peripherals/rmt-hello/board.json
````json
{ "parts": {
    "wire": { "module": "uartwire.wasm", "pins": { "RMT": 4, "TX": 5, "RX": 6 } }
} }
````

## File: peripherals/rmt-hello/README.md
````markdown
# RMT hello

The esp-hal `rmt-hello` example as an espforge project, with the UART console
showing the text the waveform carried.

The chip transmits `hello\n` on one raw RMT channel and its own UART1 receives
it, so the test proves the waveform, not just that a transmit finished:

- `<rmt id="rmt0" />` claims the chip's RMT block at its default 80 MHz.
- `<rmt_tx id="uart_tx" bus="$rmt0" pin="4" channel="0" clk_divider="80"
  idle_level="high" />` configures channel 0 the way the example configures
  its TX channel: 115200 8N1 timing, 9 ticks a bit at 1 µs a tick.
- `<uart id="uart1" uart="UART1" tx="6" rx="5" baud_rate="115200" />` is the
  receive side. `src/app.rs` prints what the port read.
- `parts/uartwire` is the wire between them, as a virtual part. The emulator
  has no way to feed an RMT channel into a UART, so the part watches the RMT
  pin as runs of edges (`reads_edges`), decodes the 8N1 bytes by sampling the
  timeline at each bit's centre, and puts them on the UART1 line (`serial`).
  `board.json` wires it: the RMT pin on GPIO 4, its TX on the chip's RX GPIO 5
  and its RX on the chip's TX GPIO 6.

| File | What it is |
|---|---|
| `rmt-hello.espf.txt` | the espforge project: `<rmt>`, `<rmt_tx>`, `<uart>` |
| `src/app.rs` | the pulse builder, the transmit, and the UART read |
| `board.json` | the virtual wire, on GPIO 4 and the UART1 line |
| `parts/uartwire/` | the part: edge-run decoding into UART bytes |

## Run it

```sh
just rmt-hello-check        # schema and resource graph
just rmt-hello-part-test    # the part's framing decode, on the host
just rmt-hello-part         # build the part -> uartwire.wasm
just rmt-hello-image        # compile -> build/rmt-hello/rmt-hello.bin (+ .elf)
just rmt-hello-run          # boot it under esp-emu for 20s
just rmt-hello-test         # gate on the console printing the received text
```

`just rmt-hello-run` prints the firmware console, including
`TEST: uart "hello"`; the emulator's own log goes to
`build/rmt-hello/emu.log`.

## Timing

| Quantity | Value |
|---|---|
| RMT source clock | 80 MHz (default for `<rmt>` on esp32c3) |
| `clk_divider` | 80, so one tick is 1 µs |
| Bit length | 9 ticks (115200 baud is 8.68 µs) |
| Idle level | high, like an idle UART line |
| Byte | low start bit, 8 data bits least significant first, high stop bit |

An RMT receiver records where the line changes, so equal neighbouring bits
arrive merged: `h`'s first three data bits are one 27 µs low. The part decodes
from the timeline, not from the transmitted pulse pairs.

## Limits

- Transmit only: no RMT receive channel and no RMT decoding in the firmware.
  The waveform is decoded by the virtual part.
- The wire is a part, not a chip feature: attach it with
  `--cosim-board board.json`, as the recipes do.
- `build/` is disposable; the fixture itself stays.
````

## File: peripherals/rmt-hello/rmt-hello.espf.txt
````
====================
RMT hello

The esp-hal `rmt-hello` example as an espforge project, read back through a
UART. One raw RMT transmit channel on GPIO 4 sends "hello\n" with 115200 8N1
timing (80 MHz / 80 = 1 MHz, 9 ticks a bit). UART1 is wired for the receive
side: its RX is on GPIO 5 and its TX on GPIO 6.

The emulator has no wire from an RMT channel to a UART, so the virtual part in
parts/uartwire is that wire: it reads GPIO 4 as runs of edges, decodes the 8N1
bytes, and puts them on the UART1 line. The app reads the port and prints what
it received, so the console shows the text the waveform carried.
====================

# espforge
  <project name="rmt_hello" />
  <chip type="esp32c3" />
  <runtime type="blocking" />

# peripherals
  <rmt id="rmt0" />
  <uart id="uart1" uart="UART1" tx="6" rx="5" baud_rate="115200" />

# devices
  <rmt_tx id="uart_tx" bus="$rmt0" pin="4" channel="0" clk_divider="80" idle_level="high" />

# services
````

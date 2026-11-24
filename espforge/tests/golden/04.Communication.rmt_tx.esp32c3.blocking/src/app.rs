//! The esp-hal `rmt_tx` example, read back through a UART.
//!
//! `uart_tx` is the raw `<rmt_tx>` channel: GPIO 4, clock divider 50, idle
//! high. At 80 MHz / 50 one tick is 625 ns, so a 115200 baud bit is 14 ticks
//! (8.75 µs, 0.8% from the ideal 8.68 µs) and a byte is a low start bit, eight
//! data bits least significant first and a high stop bit. The app builds the
//! pulses; the channel does not know UART.
//!
//! `uart1` is the chip's own second UART, its RX on GPIO 5. `diagram.json`
//! wires GPIO 4 to GPIO 5, the wire an RMT-to-UART loopback needs, so the port
//! receives what the channel transmitted. Printing what the UART read puts the
//! text on the console, where the waveform started.

use esp_hal::gpio::Level;
use espforge_runtime::PulseCode;

use crate::Context;

/// 80 MHz / 50 = 1.6 MHz (1 tick = 625 ns), 115200 baud -> 14 ticks a bit.
const BIT_TICKS: u16 = 14;

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
    {
        let tx = device!(ctx, uart_tx);
        if tx.transmit(&hello_frame()).is_err() {
            ctx.logger.error("TEST: rmt hello failed");
            return;
        }
    }

    let mut line = [0u8; LINE_LEN];
    let uart = device!(ctx, uart1);
    match uart.read_line(&mut line) {
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

//! `uart` component: a UART peripheral with a small line-buffer convenience
//! layer over `embedded-io` (ADR-003/008).
//!
//! esp-hal's `Uart` already implements `embedded_io::{Read, Write}`, so this
//! wrapper reuses them under the hood and adds the friendly helpers the
//! examples expect: `write(&str)`, `buffer_until_newline()`,
//! `get_buffered_string()`, `clear_buffer()`. `embedded_io::Write`/`Read` are
//! still reachable via `bus()`/`bus_mut()` for advanced use.

use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::uart::{Config, Uart};

/// Size of the internal line buffer (bytes). Matches wokwi's typical UART line
/// length for these course examples.
const BUF_LEN: usize = 128;

pub struct UartDevice {
    uart: Uart<'static, esp_hal::Blocking>,
    buf: [u8; BUF_LEN],
    len: usize,
}

impl UartDevice {
    /// `uart` is the UART peripheral moved in by value; `tx`/`rx` are the pins
    /// moved in by value; `baud` is the line rate in bits/sec.
    pub fn new(
        uart: esp_hal::peripherals::UART1<'static>,
        tx: impl OutputPin + 'static,
        rx: impl InputPin + 'static,
        baud: u32,
    ) -> Self {
        let config = Config::default().with_baudrate(baud);
        let uart = Uart::new(uart, config)
            .unwrap()
            .with_tx(tx)
            .with_rx(rx);
        UartDevice {
            uart,
            buf: [0u8; BUF_LEN],
            len: 0,
        }
    }

    /// Write a string, blocking until all bytes are enqueued (reuses
    /// `embedded_io::Write` under the hood).
    pub fn write(&mut self, s: &str) -> Result<(), embedded_io::ErrorKind> {
        embedded_io::Write::write_all(&mut self.uart, s.as_bytes())
            .map_err(|_| embedded_io::ErrorKind::Other)
    }

    /// Read from the UART until a `\n` (or the buffer fills), appending into the
    /// internal line buffer. Returns the number of bytes buffered so far.
    pub fn buffer_until_newline(&mut self) -> usize {
        while self.len < BUF_LEN {
            let mut byte = [0u8; 1];
            match embedded_io::Read::read_exact(&mut self.uart, &mut byte) {
                Ok(()) => {
                    self.buf[self.len] = byte[0];
                    self.len += 1;
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        self.len
    }

    /// The currently buffered line as a `&str` (valid UTF-8 prefix only).
    pub fn get_buffered_string(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// Drop the buffered line so the next `buffer_until_newline` starts fresh.
    pub fn clear_buffer(&mut self) {
        self.len = 0;
    }

    /// Shared access to the underlying esp-hal `Uart` (implements
    /// `embedded_io::{Read, Write}`).
    pub fn bus(&self) -> &Uart<'static, esp_hal::Blocking> {
        &self.uart
    }

    /// Mutable access to the underlying esp-hal `Uart`.
    pub fn bus_mut(&mut self) -> &mut Uart<'static, esp_hal::Blocking> {
        &mut self.uart
    }
}

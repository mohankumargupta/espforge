//! `uart` component: a UART peripheral with a small line-buffer convenience
//! layer over `embedded-io` (ADR-003/008).
//!
//! esp-hal's `Uart` already implements `embedded_io::{Read, Write}`, so this
//! wrapper reuses them under the hood and adds the friendly helpers the
//! examples expect: `write(&str)`, `buffer_until_newline()`,
//! `with_buffered_string(f)`, `clear_buffer()`. `embedded_io::Write`/`Read`
//! are still reachable via `with_bus(f)` for advanced use.
//!
//! State is held behind a `RefCell` (mirroring `Led`/`Button`) so every
//! method takes `&self` — this is what lets `component!(ctx, uart)` hand out
//! a shared reference that works the same way in blocking and Embassy
//! contexts. Because the internal buffer lives behind the `RefCell` guard, it
//! can no longer be returned as a borrowed `&str` (Choice A from the interior-
//! mutability writeup); callers get it via a callback instead.
 
use core::cell::RefCell;

use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::uart::{Config, Uart};

/// Size of the internal line buffer (bytes). Matches wokwi's typical UART line
/// length for these course examples.
const BUF_LEN: usize = 128;

struct Inner {
    uart: Uart<'static, esp_hal::Blocking>,
    buf: [u8; BUF_LEN],
    len: usize,
}

pub struct UartDevice {
    inner: RefCell<Inner>,
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
            inner: RefCell::new(Inner {
                uart,
                buf: [0u8; BUF_LEN],
                len: 0,
            }),
        }
    }

    /// Write a string, blocking until all bytes are enqueued (reuses
    /// `embedded_io::Write` under the hood).
    pub fn write(&self, s: &str) -> Result<(), embedded_io::ErrorKind> {
        critical_section::with(|_cs| {
            let mut inner = self.inner.borrow_mut();
            embedded_io::Write::write_all(&mut inner.uart, s.as_bytes())
                .map_err(|_| embedded_io::ErrorKind::Other)
        })
    }

    /// Read from the UART until a `\n` (or the buffer fills), appending into the
    /// internal line buffer. Returns the number of bytes buffered so far.
    pub fn buffer_until_newline(&self) -> usize {
        critical_section::with(|_cs| {
            let mut inner = self.inner.borrow_mut();
            while inner.len < BUF_LEN {
                let mut byte = [0u8; 1];
                match embedded_io::Read::read_exact(&mut inner.uart, &mut byte) {
                    Ok(()) => {
                        let idx = inner.len;
                        inner.buf[idx] = byte[0];
                        inner.len += 1;
                        if byte[0] == b'\n' {
                            break;
                        }
                    }
                }
            }
            Err(_) => break,
            inner.len
        })
    }

    /// Run `f` against the currently buffered line (valid UTF-8 prefix only).
    /// A callback is used instead of returning `&str` because the buffer lives
    /// behind the `RefCell` guard (ADR-008: interior mutability everywhere).
    pub fn with_buffered_string<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        let inner = self.inner.borrow();
        let s = core::str::from_utf8(&inner.buf[..inner.len]).unwrap_or("");
        f(s)
    }

    /// Drop the buffered line so the next `buffer_until_newline` starts fresh.
    pub fn clear_buffer(&self) {
        self.inner.borrow_mut().len = 0;
    }

    /// Run `f` against the underlying esp-hal `Uart` (implements
    /// `embedded_io::{Read, Write}`) for advanced use beyond the line-buffer
    /// helpers above.
    pub fn with_bus<R>(&self, f: impl FnOnce(&mut Uart<'static, esp_hal::Blocking>) -> R) -> R {
        critical_section::with(|_cs| f(&mut self.inner.borrow_mut().uart))
    }
}

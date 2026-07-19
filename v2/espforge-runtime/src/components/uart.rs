//! `uart` component: a UART peripheral with a line-buffer convenience layer
//! over `embedded-io` (ADR-003/008).
//!
//! Parametric over the esp-hal mode typestate `Dm` (design §20.2). `UartDevice`
//! wraps the combined `esp_hal::uart::Uart` (not split `UartTx`/`UartRx`, §20.5
//! open sub-decision resolved toward the combined form). Stream-style API
//! (`write`/`read`/`flush`, **partial `usize` writes**) per `embedded_io`, not
//! transaction-style.
//!
//! Interior mutability keyed on `Dm` (§20.3): blocking uses `RefCell`; async uses
//! `embassy_sync::Mutex<RefCell<..>>`. **The async mutex is required even though
//! UART is point-to-point** — it guards against two Embassy tasks contending on
//! the same `&'static` UART, which `component!` makes possible (§20.5).
//!
//! Line buffering uses esp-hal's `read_buffered` + a `\n` scan (§20.5), replacing
//! the old hand-rolled 128-byte `RefCell` ring. The buffer is caller-provided,
//! so the `with_line` callback is only needed when the caller does not own the
//! buffer.

use core::cell::RefCell;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::uart::{Config as EspConfig, DataBits, Parity, StopBits, Uart};
#[cfg(not(feature = "embassy"))]
use esp_hal::Blocking;
use esp_hal::DriverMode;
#[cfg(feature = "embassy")]
use esp_hal::Async;

/// Minimal YAML-facing UART config (design §20.6, Level B). `baudrate` + the
/// stable framing fields; esp-hal's unstable `baudrate_tolerance` /
/// `sw_flow_ctrl` / `hw_flow_ctrl` / `clock_source` are intentionally not
/// exposed. Bound to `embedded_io_07` (esp-hal also implements `_06`; ignored).
#[derive(Debug, Clone, Copy)]
pub struct UartConfig {
    pub baudrate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

impl Default for UartConfig {
    fn default() -> Self {
        let d = EspConfig::default();
        UartConfig {
            baudrate: d.baudrate(),
            data_bits: d.data_bits(),
            parity: d.parity(),
            stop_bits: d.stop_bits(),
        }
    }
}

impl From<UartConfig> for EspConfig {
    fn from(c: UartConfig) -> EspConfig {
        // Only baudrate is commonly varied by examples; framing defaults to
        // 8N1 (esp-hal `Config::default()`). `with_baudrate` is the
        // builder_lite setter (doc at uart/mod.rs:1974).
        let mut cfg = EspConfig::default().with_baudrate(c.baudrate);
        if c.data_bits != DataBits::_8 {
            cfg = cfg.with_data_bits(c.data_bits);
        }
        if c.parity != Parity::None {
            cfg = cfg.with_parity(c.parity);
        }
        if c.stop_bits != StopBits::_1 {
            cfg = cfg.with_stop_bits(c.stop_bits);
        }
        cfg
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UartError {
    Config(esp_hal::uart::ConfigError),
    Write(esp_hal::uart::TxError),
    Read(esp_hal::uart::RxError),
}

// ---------------------------------------------------------------------------
// One parametric `UartDevice` struct; inner sharing primitive differs by build.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
pub struct UartDevice<Dm: DriverMode + 'static> {
    inner: RefCell<Uart<'static, Dm>>,
}

#[cfg(feature = "embassy")]
pub struct UartDevice<Dm: DriverMode + 'static> {
    inner: embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        RefCell<Uart<'static, Dm>>,
    >,
}

// ---------------------------------------------------------------------------
// Blocking build + API
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl UartDevice<Blocking> {
    /// Build the owned esp-hal `Uart`. esp-hal 1.1: `new(config)` only — pins
    /// attached via `with_tx`/`with_rx`; **no `clocks` arg** (§20.1). Fallible
    /// per §20.7.
    pub fn build(
        uart: impl esp_hal::uart::Instance + 'static,
        tx: impl OutputPin + 'static,
        rx: impl InputPin + 'static,
        config: UartConfig,
    ) -> Result<Self, esp_hal::uart::ConfigError> {
        let esp = Uart::new(uart, EspConfig::from(config))?.with_tx(tx).with_rx(rx);
        Ok(UartDevice {
            inner: RefCell::new(esp),
        })
    }

    /// Write all bytes, looping over esp-hal's partial `write` (returns usize).
    pub fn write_all(&self, data: &[u8]) -> Result<(), UartError> {
        let mut uart = self.inner.borrow_mut();
        let mut off = 0;
        while off < data.len() {
            let n = uart.write(&data[off..]).map_err(UartError::Write)?;
            off += n;
        }
        uart.flush().map_err(UartError::Write)
    }

    pub fn write_str(&self, s: &str) -> Result<(), UartError> {
        self.write_all(s.as_bytes())
    }

    /// Read into `buf`; returns bytes read (may be < `buf.len()`).
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner.borrow_mut().read(buf).map_err(UartError::Read)
    }

    /// Read available bytes via esp-hal's buffered read (§20.5).
    pub fn read_buffered(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner
            .borrow_mut()
            .read_buffered(buf)
            .map_err(UartError::Read)
    }

    /// Read until `\n` (or `buf` full). Returns total bytes read. Backed by
    /// `read_buffered` + `\n` scan; replaces the old hand-rolled ring (§20.5).
    pub fn read_line(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        let mut total = 0;
        while total < buf.len() {
            let n = self.read_buffered(&mut buf[total..])?;
            if n == 0 {
                if total > 0 && buf[..total].contains(&b'\n') {
                    break;
                }
                esp_hal::delay::Delay::new().delay_millis(1);
                continue;
            }
            total += n;
            if buf[..total].contains(&b'\n') {
                break;
            }
        }
        Ok(total)
    }

    /// Callback access to a line (caller does not own the buffer).
    pub fn with_line<R>(
        &self,
        buf: &mut [u8],
        f: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, UartError> {
        let n = self.read_line(buf)?;
        Ok(f(&buf[..n]))
    }
}

// ---------------------------------------------------------------------------
// Async build + API (only under `embassy`, §20.3/§20.4)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
impl UartDevice<Async> {
    pub fn build(
        uart: impl esp_hal::uart::Instance + 'static,
        tx: impl OutputPin + 'static,
        rx: impl InputPin + 'static,
        config: UartConfig,
    ) -> Result<Self, esp_hal::uart::ConfigError> {
        let esp = Uart::new(uart, EspConfig::from(config))?
            .with_tx(tx)
            .with_rx(rx)
            .into_async();
        Ok(UartDevice {
            inner: embassy_sync::mutex::Mutex::new(RefCell::new(esp)),
        })
    }

    pub async fn write_all(&self, data: &[u8]) -> Result<(), UartError> {
        let mut uart = self.inner.lock().await;
        let mut off = 0;
        while off < data.len() {
            let n = uart
                .borrow_mut()
                .write(&data[off..])
                .await
                .map_err(UartError::Write)?;
            off += n;
        }
        uart.borrow_mut().flush().await.map_err(UartError::Write)
    }

    pub async fn write_str(&self, s: &str) -> Result<(), UartError> {
        self.write_all(s.as_bytes()).await
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner
            .lock()
            .await
            .borrow_mut()
            .read(buf)
            .await
            .map_err(UartError::Read)
    }

    pub async fn read_buffered(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner
            .lock()
            .await
            .borrow_mut()
            .read_buffered(buf)
            .await
            .map_err(UartError::Read)
    }

    pub async fn read_line(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        let mut total = 0;
        while total < buf.len() {
            // Release the lock between idle reads (§20.3: not held across await).
            let n = {
                let mut uart = self.inner.lock().await;
                uart.borrow_mut()
                    .read_buffered(&mut buf[total..])
                    .await
                    .map_err(UartError::Read)?
            };
            if n == 0 {
                if total > 0 && buf[..total].contains(&b'\n') {
                    break;
                }
                embassy_time::Timer::after(embassy_time::Duration::from_millis(1)).await;
                continue;
            }
            total += n;
            if buf[..total].contains(&b'\n') {
                break;
            }
        }
        Ok(total)
    }

    pub async fn with_line<R>(
        &self,
        buf: &mut [u8],
        f: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, UartError> {
        let n = self.read_line(buf).await?;
        Ok(f(&buf[..n]))
    }
}

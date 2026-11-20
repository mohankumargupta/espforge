//! `UartDevice`: a UART peripheral with a line-buffer convenience layer.
//!
//! Stream-style API (`write`/`read`/`flush`, partial `usize` writes) per
//! `embedded_io`. Interior mutability is keyed on `Dm`: blocking uses a
//! `RefCell`, async an `embassy_sync::Mutex`, so a shared `&'static Context`
//! can drive the port.
//!
//! Line buffering uses esp-hal's `read_buffered` plus a `\n` scan; the buffer
//! is caller-provided.
//!
//! The standard stream traits are implemented on the device itself:
//! `embedded_io::Read + Write` for the blocking port and, under `embassy`,
//! `embedded_io_async::Read + Write` for the async one. A crates.io driver
//! generic over those traits therefore accepts the port directly, which is what
//! lets a UART part be a manifest-only driver. The inherent `read`/`write_all`
//! stay: same names, and they shadow the trait methods for direct calls.

// Only the blocking port parks its hardware in a `RefCell`; the async one uses
// an embassy mutex.
#[cfg(not(feature = "embassy"))]
use core::cell::RefCell;
#[cfg(feature = "embassy")]
use esp_hal::Async;
#[cfg(not(feature = "embassy"))]
use esp_hal::Blocking;
use esp_hal::DriverMode;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::uart::{Config as EspConfig, DataBits, Instance, Parity, StopBits, Uart};

/// Minimal UART configuration: baudrate plus the stable framing fields.
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
        // 8N1 (esp-hal `Config::default()`).
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

// `embedded_io 0.7`'s `Error` is a stronger trait than the 0.6 one: it
// requires `core::error::Error`, which is `Debug + Display`. The stream traits
// carry `UartError` because the esp-hal error types are foreign (the orphan
// rule forbids implementing `embedded_io`'s error traits for them here).
impl core::fmt::Display for UartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UartError::Config(error) => write!(f, "uart configuration failed: {error:?}"),
            UartError::Write(error) => write!(f, "uart write failed: {error:?}"),
            UartError::Read(error) => write!(f, "uart read failed: {error:?}"),
        }
    }
}

impl core::error::Error for UartError {}

impl embedded_io::Error for UartError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            // Construction happened in `build`, so a reader only sees this if
            // the device was reused across a reconfiguration.
            UartError::Config(_) => embedded_io::ErrorKind::InvalidInput,
            UartError::Write(_) | UartError::Read(_) => embedded_io::ErrorKind::Other,
        }
    }
}

#[cfg(not(feature = "embassy"))]
pub struct UartDevice<Dm: DriverMode + 'static> {
    inner: RefCell<Uart<'static, Dm>>,
}

#[cfg(feature = "embassy")]
pub struct UartDevice<Dm: DriverMode + 'static> {
    inner: embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Uart<'static, Dm>,
    >,
}

// ---------------------------------------------------------------------------
// Blocking build + API
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl UartDevice<Blocking> {
    pub fn build<I: Instance + 'static>(
        uart: I,
        tx: impl OutputPin + 'static,
        rx: impl InputPin + 'static,
        config: UartConfig,
    ) -> Result<Self, esp_hal::uart::ConfigError> {
        let esp = Uart::new(uart, EspConfig::from(config))?
            .with_tx(tx)
            .with_rx(rx);
        Ok(UartDevice {
            inner: RefCell::new(esp),
        })
    }

    /// Write all bytes, looping over esp-hal's partial `write`.
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

    pub fn read_buffered(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner
            .borrow_mut()
            .read_buffered(buf)
            .map_err(UartError::Read)
    }

    /// Read until `\n` (or `buf` full). Returns total bytes read.
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
    pub fn with_line<R>(&self, buf: &mut [u8], f: impl FnOnce(&[u8]) -> R) -> Result<R, UartError> {
        let n = self.read_line(buf)?;
        Ok(f(&buf[..n]))
    }
}

// ---------------------------------------------------------------------------
// Blocking standard stream traits
// ---------------------------------------------------------------------------

#[cfg(not(feature = "embassy"))]
impl embedded_io::ErrorType for UartDevice<Blocking> {
    type Error = UartError;
}

#[cfg(not(feature = "embassy"))]
impl embedded_io::Read for UartDevice<Blocking> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        // `embedded-io` reserves `Ok(0)` for "no progress"; a UART has no EOF,
        // so an idle port waits for the first byte instead of reporting one.
        // The inherent `read` is the one called here: the trait method is the
        // same name on this type.
        loop {
            let n = self.inner.borrow_mut().read(buf).map_err(UartError::Read)?;
            if n > 0 {
                return Ok(n);
            }
            esp_hal::delay::Delay::new().delay_millis(1);
        }
    }
}

#[cfg(not(feature = "embassy"))]
impl embedded_io::Write for UartDevice<Blocking> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner.borrow_mut().write(buf).map_err(UartError::Write)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.borrow_mut().flush().map_err(UartError::Write)
    }
}

// ---------------------------------------------------------------------------
// Async build + API (only under `embassy`)
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy")]
impl UartDevice<Async> {
    pub fn build<I: Instance + 'static>(
        uart: I,
        tx: impl OutputPin + 'static,
        rx: impl InputPin + 'static,
        config: UartConfig,
    ) -> Result<Self, esp_hal::uart::ConfigError> {
        let esp = Uart::new(uart, EspConfig::from(config))?
            .with_tx(tx)
            .with_rx(rx)
            .into_async();
        Ok(UartDevice {
            inner: embassy_sync::mutex::Mutex::new(esp),
        })
    }

    /// The handle `device!` awaits.
    ///
    /// A UART is point-to-point but not exclusive: every operation already
    /// takes this port's own mutex, and `read_line` releases it between idle
    /// reads. Wrapping the port in the board's `AsyncMutex` as well would hold
    /// a second lock across those awaits and starve every other task using the
    /// port, so the macro gets a shared handle instead.
    pub async fn lock(&self) -> &Self {
        self
    }

    pub async fn write_all(&self, data: &[u8]) -> Result<(), UartError> {
        let mut uart = self.inner.lock().await;
        let mut off = 0;
        while off < data.len() {
            let n = uart.write(&data[off..]).map_err(UartError::Write)?;
            off += n;
        }
        uart.flush().map_err(UartError::Write)
    }

    pub async fn write_str(&self, s: &str) -> Result<(), UartError> {
        self.write_all(s.as_bytes()).await
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner.lock().await.read(buf).map_err(UartError::Read)
    }

    pub async fn read_buffered(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        self.inner
            .lock()
            .await
            .read_buffered(buf)
            .map_err(UartError::Read)
    }

    pub async fn read_line(&self, buf: &mut [u8]) -> Result<usize, UartError> {
        let mut total = 0;
        while total < buf.len() {
            // Release the lock between idle reads: not held across await.
            let n = {
                let mut uart = self.inner.lock().await;
                uart.read_buffered(&mut buf[total..])
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

// ---------------------------------------------------------------------------
// Async standard stream traits
// ---------------------------------------------------------------------------

// These delegate to esp-hal's `*_async` methods rather than to the synchronous
// ones the inherent API uses: `read_async` waits for data instead of returning
// `0`, so a reader yields to the executor rather than busy-polling a timer.
// The guard is held across one await; the port has a single owner, so that is
// what serialises access to it.

#[cfg(feature = "embassy")]
impl embedded_io::ErrorType for UartDevice<Async> {
    type Error = UartError;
}

#[cfg(feature = "embassy")]
impl embedded_io_async::Read for UartDevice<Async> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.inner
            .lock()
            .await
            .read_async(buf)
            .await
            .map_err(UartError::Read)
    }
}

#[cfg(feature = "embassy")]
impl embedded_io_async::Write for UartDevice<Async> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner
            .lock()
            .await
            .write_async(buf)
            .await
            .map_err(UartError::Write)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner
            .lock()
            .await
            .flush_async()
            .await
            .map_err(UartError::Write)
    }
}

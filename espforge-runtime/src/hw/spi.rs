//! `SpiBus`: an SPI master bus shared by reference by its devices.
//!
//! Two flavours, and the scheduler no longer decides which one exists:
//!
//! - [`SpiBus`] is the blocking bus, parked in a `critical_section::Mutex`. It
//!   is compiled in every build, including an embassy project: a display on an
//!   embassy board still talks over a blocking bus, before the executor
//!   starts.
//! - [`AsyncSpiBus`] is the async bus (feature `embassy`).
//!
//! Both bus handles are `Send + Sync`, so a device that owns one can live in
//! an `AsyncMutex` on an embassy board. A device endpoint is
//! [`ConfiguredSpiDevice`] (or [`ConfiguredAsyncSpiDevice`]): the shared bus
//! plus the part's own chip select, configuration and polarity. There is
//! deliberately one endpoint per mode; a raw `<spi cs="...">` bus exposes the
//! same type, so no device type has a second, unconfigured sibling.
//!
//! `build_in` is generic over the peripheral instance, so the runtime does not
//! hardcode `SPI2`; the generated tokens still spell `SPI2`. The storage lives
//! in a [`SpiBusStorage`] (or [`AsyncSpiBusStorage`]) the *generated* code
//! declares, one `static` per `<spi>` element, so two buses never share one
//! cell.

use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
use esp_hal::Blocking;
use esp_hal::gpio::{InputPin, Output, OutputPin};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as EspConfig, Instance, Spi};

use crate::Delay;

/// Minimal SPI configuration: `frequency` + `mode` only.
#[derive(Debug, Clone, Copy)]
pub struct SpiConfig {
    pub frequency: esp_hal::time::Rate,
    pub mode: Mode,
}

impl SpiConfig {
    /// The comparison key the bus remembers: `(kHz, mode)`.
    ///
    /// `Rate` and `Mode` do not implement `PartialEq`, and two configs are the
    /// same transfer setup exactly when these two numbers agree.
    fn key(self) -> (u32, u8) {
        (self.frequency.as_khz(), mode_number(self.mode))
    }
}

/// No configuration has been applied to a controller yet.
const NO_CONFIG: u32 = u32::MAX;

/// The packed `(kHz, mode)` key, small enough for an atomic.
///
/// The key lives in a `static` the generated code owns, and an atomic is the
/// interior mutability that keeps that static `Sync`; `Rate` and `Mode` are
/// not `PartialEq`, so the two numbers are packed instead of compared.
fn packed(config: SpiConfig) -> u32 {
    let (khz, mode) = config.key();
    (khz << 4) | u32::from(mode)
}

/// The `Mode` variant as the number the project wrote (`0..=3`).
pub fn mode_number(mode: Mode) -> u8 {
    match mode {
        Mode::_0 => 0,
        Mode::_1 => 1,
        Mode::_2 => 2,
        Mode::_3 => 3,
    }
}

/// Which level asserts a device's chip select.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CsPolarity {
    /// The common case: the part listens while CS is low.
    #[default]
    ActiveLow,
    /// A part that listens while CS is high, carried by the same adapter.
    ActiveHigh,
}

impl CsPolarity {
    /// The level that selects the part.
    pub fn asserted(self) -> esp_hal::gpio::Level {
        match self {
            CsPolarity::ActiveLow => esp_hal::gpio::Level::Low,
            CsPolarity::ActiveHigh => esp_hal::gpio::Level::High,
        }
    }

    /// The idle level, the opposite of [`asserted`](Self::asserted).
    pub fn idle(self) -> esp_hal::gpio::Level {
        match self {
            CsPolarity::ActiveLow => esp_hal::gpio::Level::High,
            CsPolarity::ActiveHigh => esp_hal::gpio::Level::Low,
        }
    }
}

impl Default for SpiConfig {
    fn default() -> Self {
        SpiConfig {
            frequency: esp_hal::time::Rate::from_mhz(1),
            mode: Mode::_0,
        }
    }
}

impl From<SpiConfig> for EspConfig {
    fn from(c: SpiConfig) -> EspConfig {
        EspConfig::default()
            .with_frequency(c.frequency)
            .with_mode(c.mode)
    }
}

/// Unified SPI error surfaced to examples. (`ConfigError` lacks `Eq`, and the
/// esp-hal bus `Error` is private, so `Bus` carries no payload.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpiError {
    Config(esp_hal::spi::master::ConfigError),
    Bus,
}

impl embedded_hal::spi::Error for SpiError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

// ---------------------------------------------------------------------------
// Blocking bus (every build)
// ---------------------------------------------------------------------------

/// The cell one blocking `<spi>` bus lives in.
///
/// The critical section is what makes the bus handle `Send + Sync`: a bare
/// `RefCell` is neither, and an embassy board stores blocking devices inside
/// an `AsyncMutex`.
type BusCell = critical_section::Mutex<RefCell<Spi<'static, Blocking>>>;

pub struct SpiBus {
    inner: &'static BusCell,
    /// The last configuration applied to the controller, owned by the bus so
    /// two devices alternating cannot leave a stale "already applied" flag on
    /// an endpoint. An atomic keeps the handle `Copy` and the storage `Sync`.
    applied: &'static AtomicU32,
}

impl Clone for SpiBus {
    fn clone(&self) -> Self {
        *self
    }
}
impl Copy for SpiBus {}

/// The `static` one blocking `<spi>` bus needs.
///
/// The generated `generated.rs` declares one per bus (`__SPI2_STORAGE`), so
/// [`SpiBus::build_in`] and [`SpiBus::build_write_only_in`] can never
/// initialise the same cell twice.
pub struct SpiBusStorage {
    cell: static_cell::StaticCell<BusCell>,
    applied: AtomicU32,
}

impl SpiBusStorage {
    pub const fn new() -> Self {
        Self {
            cell: static_cell::StaticCell::new(),
            applied: AtomicU32::new(NO_CONFIG),
        }
    }
}

impl Default for SpiBusStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl SpiBus {
    pub fn build_in<I: Instance + 'static>(
        storage: &'static SpiBusStorage,
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus, esp_hal::spi::master::ConfigError> {
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso);
        let inner = storage
            .cell
            .init(critical_section::Mutex::new(RefCell::new(esp)));
        storage.applied.store(packed(config), Ordering::Relaxed);
        Ok(SpiBus {
            inner,
            applied: &storage.applied,
        })
    }

    /// Build a bus with no MISO pin: a display or other write-only device.
    pub fn build_write_only_in<I: Instance + 'static>(
        storage: &'static SpiBusStorage,
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        config: SpiConfig,
    ) -> Result<SpiBus, esp_hal::spi::master::ConfigError> {
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi);
        let inner = storage
            .cell
            .init(critical_section::Mutex::new(RefCell::new(esp)));
        storage.applied.store(packed(config), Ordering::Relaxed);
        Ok(SpiBus {
            inner,
            applied: &storage.applied,
        })
    }

    /// Apply `config` to the controller if it differs from the last one.
    ///
    /// A device's own mode and frequency are applied before its chip select is
    /// asserted, so two parts on one controller never inherit each other's
    /// clock setup. A call that changes nothing performs no bus traffic.
    pub fn configure(&self, config: SpiConfig) -> Result<(), SpiError> {
        let key = packed(config);
        if self.applied.load(Ordering::Relaxed) == key {
            return Ok(());
        }
        critical_section::with(|cs| {
            let mut bus = self.inner.borrow(cs).borrow_mut();
            bus.apply_config(&EspConfig::from(config))
        })
        .map_err(SpiError::Config)?;
        self.applied.store(key, Ordering::Relaxed);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Async bus (only under `embassy`)
// ---------------------------------------------------------------------------

/// The `static` one async `<spi>` bus needs: one mutex-armoured cell per
/// `<spi>` element.
#[cfg(feature = "embassy")]
pub struct AsyncSpiBusStorage {
    cell: static_cell::StaticCell<
        embassy_sync::mutex::Mutex<
            embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
            Spi<'static, esp_hal::Async>,
        >,
    >,
    applied: AtomicU32,
}

#[cfg(feature = "embassy")]
impl AsyncSpiBusStorage {
    pub const fn new() -> Self {
        Self {
            cell: static_cell::StaticCell::new(),
            applied: AtomicU32::new(NO_CONFIG),
        }
    }
}

#[cfg(feature = "embassy")]
impl Default for AsyncSpiBusStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "embassy")]
pub struct AsyncSpiBus {
    inner: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        Spi<'static, esp_hal::Async>,
    >,
    /// The last configuration applied to the controller, shared by every
    /// endpoint on the bus. Mirrors [`SpiBus::applied`]: without it, an async
    /// endpoint reapplied its config on every transaction.
    applied: &'static AtomicU32,
}

#[cfg(feature = "embassy")]
impl Clone for AsyncSpiBus {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(feature = "embassy")]
impl Copy for AsyncSpiBus {}

#[cfg(feature = "embassy")]
impl AsyncSpiBus {
    pub fn build_in<I: Instance + 'static>(
        storage: &'static AsyncSpiBusStorage,
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        config: SpiConfig,
    ) -> Result<AsyncSpiBus, esp_hal::spi::master::ConfigError> {
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso)
            .into_async();
        let inner = storage.cell.init(embassy_sync::mutex::Mutex::new(esp));
        storage.applied.store(packed(config), Ordering::Relaxed);
        Ok(AsyncSpiBus {
            inner,
            applied: &storage.applied,
        })
    }

    pub fn build_write_only_in<I: Instance + 'static>(
        storage: &'static AsyncSpiBusStorage,
        spi: I,
        sclk: impl OutputPin + 'static,
        mosi: impl OutputPin + 'static,
        config: SpiConfig,
    ) -> Result<AsyncSpiBus, esp_hal::spi::master::ConfigError> {
        let esp = Spi::new(spi, EspConfig::from(config))?
            .with_sck(sclk)
            .with_mosi(mosi)
            .into_async();
        let inner = storage.cell.init(embassy_sync::mutex::Mutex::new(esp));
        storage.applied.store(packed(config), Ordering::Relaxed);
        Ok(AsyncSpiBus {
            inner,
            applied: &storage.applied,
        })
    }

    /// Apply `config` to the controller if it differs from the last one.
    ///
    /// The async mirror of [`SpiBus::configure`]: a device alternating between
    /// two setups pays for the change, and a transaction that needs nothing new
    /// performs no extra bus traffic.
    pub async fn configure(&self, config: SpiConfig) -> Result<(), SpiError> {
        let key = packed(config);
        if self.applied.load(Ordering::Relaxed) == key {
            return Ok(());
        }
        let mut bus = self.inner.lock().await;
        apply_config_locked(&mut bus, self.applied, config)
    }
}

// ---------------------------------------------------------------------------
// Device tier: per-device config and chip-select polarity
// ---------------------------------------------------------------------------

/// A device endpoint on a shared blocking SPI bus, owning its chip select.
///
/// This replaces `embedded-hal-bus`'s `RefCellDevice` for the **device** tier
/// only: that adapter has no per-transaction configuration and no way to say
/// a part's select line is active-high. Each transaction applies the device's
/// own [`SpiConfig`] before asserting CS, runs the operations, and releases CS
/// in the opposite direction. `transaction` takes `&mut self`, so the CS line
/// is a plain field: two transactions on one endpoint cannot interleave.
pub struct ConfiguredSpiDevice {
    bus: SpiBus,
    cs: Output<'static>,
    delay: Delay,
    config: SpiConfig,
    polarity: CsPolarity,
}

impl ConfiguredSpiDevice {
    /// Attach a part with its own chip select, configuration and polarity to
    /// an already-built bus.
    pub fn new(
        bus: SpiBus,
        cs: Output<'static>,
        delay: Delay,
        config: SpiConfig,
        polarity: CsPolarity,
    ) -> Self {
        Self {
            bus,
            cs,
            delay,
            config,
            polarity,
        }
    }

    /// The configuration this part applies before each transaction.
    pub fn config(&self) -> SpiConfig {
        self.config
    }

    /// The chip-select polarity this part was built with.
    pub fn polarity(&self) -> CsPolarity {
        self.polarity
    }

    pub fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        // Apply the part's mode and clock while its chip select is still
        // deasserted. `apply_config` can change CPOL/CPHA, and a clock edge
        // that reaches a selected slave is a phantom bit: the part latches a
        // byte before its first real transfer. A configuration failure returns
        // here, with CS untouched.
        self.bus.configure(self.config)?;
        self.cs.set_level(self.polarity.asserted());
        let result = critical_section::with(|cs| {
            let mut bus = self.bus.inner.borrow(cs).borrow_mut();
            run_ops(&mut *bus, &self.delay, operations)
        });
        self.cs.set_level(self.polarity.idle());
        result
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Write(bytes)])
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Read(buf)])
    }

    pub fn write_read(&mut self, write: &[u8], read: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [
            embedded_hal::spi::Operation::Write(write),
            embedded_hal::spi::Operation::Read(read),
        ])
    }

    pub fn transfer_in_place(&mut self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::TransferInPlace(buf)])
    }
}

fn run_ops(
    bus: &mut Spi<'static, Blocking>,
    delay: &Delay,
    operations: &mut [embedded_hal::spi::Operation<'_, u8>],
) -> Result<(), SpiError> {
    use embedded_hal::spi::{Operation, SpiBus};
    for op in operations.iter_mut() {
        match op {
            Operation::Read(buf) => bus.read(buf).map_err(|_| SpiError::Bus)?,
            Operation::Write(buf) => bus.write(buf).map_err(|_| SpiError::Bus)?,
            // esp-hal's inherent `transfer` is in-place and shadows the
            // trait, so reach the two-buffer method via UFCS.
            Operation::Transfer(read, write) => {
                <Spi<'_, Blocking> as SpiBus>::transfer(bus, read, write)
                    .map_err(|_| SpiError::Bus)?
            }
            Operation::TransferInPlace(buf) => {
                bus.transfer_in_place(buf).map_err(|_| SpiError::Bus)?
            }
            Operation::DelayNs(ns) => {
                delay.delay_ns((*ns).try_into().unwrap_or(u32::MAX));
            }
        }
    }
    Ok(())
}

/// A device endpoint on a shared async SPI bus, owning its chip select.
///
/// The async mirror of [`ConfiguredSpiDevice`]: each transaction applies the
/// part's own configuration, asserts CS across every `.await`, and releases it
/// afterwards. The CS line is a plain field the adapter owns, so no other task
/// can release it while a transaction is in flight; the bus itself is shared
/// through [`AsyncSpiBus`]'s mutex.
#[cfg(feature = "embassy")]
pub struct ConfiguredAsyncSpiDevice {
    bus: AsyncSpiBus,
    cs: Output<'static>,
    delay: Delay,
    config: SpiConfig,
    polarity: CsPolarity,
}

#[cfg(feature = "embassy")]
impl ConfiguredAsyncSpiDevice {
    /// Attach a part with its own chip select, configuration and polarity to
    /// an already-built async bus.
    pub fn new(
        bus: AsyncSpiBus,
        cs: Output<'static>,
        delay: Delay,
        config: SpiConfig,
        polarity: CsPolarity,
    ) -> Self {
        Self {
            bus,
            cs,
            delay,
            config,
            polarity,
        }
    }

    /// The configuration this part applies before each transaction.
    pub fn config(&self) -> SpiConfig {
        self.config
    }

    /// The chip-select polarity this part was built with.
    pub fn polarity(&self) -> CsPolarity {
        self.polarity
    }

    pub async fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), SpiError> {
        // One lock covers the configuration change and the transfer. Two parts
        // with different modes share this bus: releasing the lock between
        // `configure` and the operations would let another task apply its own
        // mode while this device's CS is asserted.
        let inner = self.bus.inner;
        let applied = self.bus.applied;
        let mut bus = inner.lock().await;
        // As on the blocking device, configure before asserting CS so a clock
        // polarity change never reaches a selected slave. A failure returns
        // with CS untouched.
        apply_config_locked(&mut bus, applied, self.config)?;
        self.cs.set_level(self.polarity.asserted());
        let result = run_ops_async(&mut bus, &self.delay, operations).await;
        self.cs.set_level(self.polarity.idle());
        result
    }

    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Write(bytes)])
            .await
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::Read(buf)])
            .await
    }

    pub async fn write_read(&mut self, write: &[u8], read: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [
            embedded_hal::spi::Operation::Write(write),
            embedded_hal::spi::Operation::Read(read),
        ])
        .await
    }

    pub async fn transfer_in_place(&mut self, buf: &mut [u8]) -> Result<(), SpiError> {
        self.transaction(&mut [embedded_hal::spi::Operation::TransferInPlace(buf)])
            .await
    }
}

/// Apply `config` to a locked controller, updating the bus's shared key.
///
/// Takes the guard's contents rather than the handle so the device's
/// transaction can hold **one** lock across the configuration change and the
/// operations: a second acquisition would let another task reconfigure the
/// bus between the two.
#[cfg(feature = "embassy")]
fn apply_config_locked(
    bus: &mut Spi<'static, esp_hal::Async>,
    applied: &AtomicU32,
    config: SpiConfig,
) -> Result<(), SpiError> {
    let key = packed(config);
    if applied.load(Ordering::Relaxed) == key {
        return Ok(());
    }
    bus.apply_config(&EspConfig::from(config))
        .map_err(SpiError::Config)?;
    applied.store(key, Ordering::Relaxed);
    Ok(())
}

#[cfg(feature = "embassy")]
async fn run_ops_async(
    bus: &mut Spi<'static, esp_hal::Async>,
    delay: &Delay,
    operations: &mut [embedded_hal::spi::Operation<'_, u8>],
) -> Result<(), SpiError> {
    for op in operations.iter_mut() {
        match op {
            embedded_hal::spi::Operation::Read(buf) => {
                embedded_hal_async::spi::SpiBus::read(&mut *bus, buf)
                    .await
                    .map_err(|_| SpiError::Bus)?
            }
            embedded_hal::spi::Operation::Write(buf) => {
                embedded_hal_async::spi::SpiBus::write(&mut *bus, buf)
                    .await
                    .map_err(|_| SpiError::Bus)?
            }
            embedded_hal::spi::Operation::Transfer(read, write) => {
                embedded_hal_async::spi::SpiBus::transfer(&mut *bus, read, write)
                    .await
                    .map_err(|_| SpiError::Bus)?
            }
            embedded_hal::spi::Operation::TransferInPlace(buf) => {
                embedded_hal_async::spi::SpiBus::transfer_in_place(&mut *bus, buf)
                    .await
                    .map_err(|_| SpiError::Bus)?
            }
            embedded_hal::spi::Operation::DelayNs(ns) => {
                delay.delay_ns((*ns).try_into().unwrap_or(u32::MAX));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "embassy")]
impl embedded_hal::spi::ErrorType for ConfiguredAsyncSpiDevice {
    type Error = SpiError;
}

#[cfg(feature = "embassy")]
impl embedded_hal_async::spi::SpiDevice for ConfiguredAsyncSpiDevice {
    async fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        ConfiguredAsyncSpiDevice::transaction(self, operations).await
    }
}

impl embedded_hal::spi::ErrorType for ConfiguredSpiDevice {
    type Error = SpiError;
}

impl embedded_hal::spi::SpiDevice for ConfiguredSpiDevice {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        ConfiguredSpiDevice::transaction(self, operations)
    }
}

//! RMT pulse channels (`<rmt>`, `<rmt_tx>`, `<rmt_rx>`).
//!
//! This module is behind the `rmt` cargo feature, not a chip cfg: build-script
//! cfgs are visible only inside `esp-hal`, and `espforge-runtime` declares no
//! chip feature of its own. The generated project enables `rmt` when the board
//! declares any RMT element; `esp32c2`/`esp32c61` projects never get that far
//! (`<rmt>` is refused with `E0530`), so `esp_hal::rmt` is never referenced on
//! a chip that does not have it.
//!
//! # `esp-hal 1.2.2` contract
//!
//! Recorded from `esp-hal-1.2.2/src/rmt.rs` before the element and endpoint
//! work started. Every later RMT module is written against this table.
//!
//! | Question | `esp-hal 1.2.2` answer |
//! |---|---|
//! | `Rmt::new` | `Rmt::<'rmt, Blocking>::new(peripheral: RMT<'rmt>, frequency: Rate) -> Result<Rmt<'rmt, Blocking>, ConfigError>` |
//! | `into_async` | on `Rmt<'rmt, Blocking>`, consuming it: `fn into_async(self) -> Rmt<'rmt, Async>` |
//! | Channel access | public fields `rmt.channel0`, `rmt.channel1`, … of type `ChannelCreator<'ch, Dm, N>` |
//! | `configure_tx` | `TxChannelCreator::configure_tx(self, config: &TxChannelConfig) -> Result<Channel<'ch, Dm, Tx>, ConfigError>` |
//! | `configure_rx` | `RxChannelCreator::configure_rx(self, config: &RxChannelConfig) -> Result<Channel<'ch, Dm, Rx>, ConfigError>` |
//! | Channel index in the type | **no**: `Channel<'ch, Dm, Dir>`, so the four endpoint aliases are writable |
//! | `with_pin` | a builder method on `Channel` (`Tx` takes `impl PeripheralOutput`, `Rx` takes `impl PeripheralInput`), not a config field |
//! | `PulseCode` | `new(level1: Level, length1: u16, level2: Level, length2: u16)` + `end_marker()`; lengths are 15-bit (`MAX_LEN = 0x7FFF`), empty halves mean end marker |
//! | Blocking TX | `Channel<'_, Blocking, Tx>::transmit(self, &[PulseCode]) -> Result<TxTransaction, (Error, Self)>`; `.wait() -> Result<Channel, (Error, Channel)>` |
//! | Blocking RX | `Channel<'_, Blocking, Rx>::receive(self, &mut [PulseCode]) -> Result<RxTransaction, (Error, Self)>`; `.wait() -> Result<(usize, Channel), (Error, Channel)>` |
//! | Async TX | `Channel<'_, Async, Tx>::transmit(&mut self, &[PulseCode]) -> impl Future<Output = Result<(), Error>>` — borrows, no `Option` dance |
//! | Async RX | `Channel<'_, Async, Rx>::receive(&mut self, &mut [PulseCode]) -> impl Future<Output = Result<usize, Error>>` |
//! | Error type | `esp_hal::rmt::Error` (non-exhaustive enum); aliased below as [`RmtError`] |
//! | Driver mode | `esp_hal::{Blocking, Async}` implement `esp_hal::DriverMode` (one mode for the whole `Rmt`) |
//! | Carrier | `TxChannelConfig::with_carrier_modulation(bool)`, `with_carrier_high(u16)`, `with_carrier_low(u16)`, `with_carrier_level(Level)` |
//!
//! Because `into_async` consumes the whole `Rmt` and the channel creators are
//! plain fields, a board resolves **one** [`crate::BusMode`]-equivalent mode per
//! RMT block: a blocking channel moved out of an async `Rmt` does not exist.
//!
//! `--features rmt` on `esp32c2` is expected to fail: that chip has no
//! `esp_hal::rmt` module or `RMT` peripheral, so the check reports `cannot
//! find rmt in esp_hal` (E0433) and `cannot find type RMT` (E0425), not a
//! runtime failure. The generated project cannot reach this state because lowering
//! refuses `<rmt>` there first; it is recorded here, not tested.
//!
//! No `static` may live in this module (`no_static_storage_in_runtime`): RX
//! buffers and channels belong to the caller/element, never to a hidden cell.
//!
//! # Endpoints
//!
//! [`RmtTxChannel`] and [`RmtRxChannel`] own a configured channel and expose
//! the two operations a raw channel needs. The blocking form must put the
//! channel back after every transaction because `esp-hal`'s blocking
//! `transmit`/`receive` consume it; the async form borrows `&mut self`, so it
//! needs no `Option` dance. The C6 aliases below are the only names an element
//! or manifest may use for these endpoints.

use esp_hal::DriverMode;
use esp_hal::rmt::{Channel, Rx, Tx};
use esp_hal::time::Rate;

pub use esp_hal::rmt::{PulseCode, RxChannelConfig, TxChannelConfig};

/// The RMT driver's error, named once so a manifest never spells
/// `esp_hal::rmt::Error`.
pub type RmtError = esp_hal::rmt::Error;

/// A configured RMT transmit channel.
///
/// The blocking half is an `Option` because `esp-hal`'s blocking `transmit`
/// consumes the channel and returns it from `.wait()`; the async half uses the
/// same field but only ever borrows it. Only one `Dm` impl block is legal per
/// mode, so both halves can spell the method `transmit`.
pub struct RmtTxChannel<Dm: DriverMode> {
    channel: Option<Channel<'static, Dm, Tx>>,
    /// The divided channel clock the channel was configured at, so a driver can
    /// turn a datasheet timing into ticks without guessing the chip.
    clock_hz: u32,
}

impl<Dm: DriverMode> RmtTxChannel<Dm> {
    /// Wrap an already configured channel.
    ///
    /// The clock defaults to 80 MHz: the pre-RMT-clock constructor. Generated
    /// code uses [`Self::with_clock`] so a 32 MHz `esp32h2` or a non-default
    /// `<rmt frequency>` is reflected in timing calculations.
    pub fn new(channel: Channel<'static, Dm, Tx>) -> Self {
        Self::with_clock(channel, 80_000, 1)
    }

    /// Wrap a channel and record its source clock and divider.
    pub fn with_clock(channel: Channel<'static, Dm, Tx>, source_khz: u32, clk_divider: u8) -> Self {
        Self {
            channel: Some(channel),
            clock_hz: source_khz.saturating_mul(1_000) / u32::from(clk_divider.max(1)),
        }
    }

    /// The channel's divided clock, in Hz.
    pub fn clock_hz(&self) -> u32 {
        self.clock_hz
    }
}

impl RmtTxChannel<esp_hal::Blocking> {
    /// Transmit one pulse-code sequence and wait for it to finish.
    pub fn transmit(&mut self, data: &[PulseCode]) -> Result<(), RmtError> {
        let channel = self
            .channel
            .take()
            .expect("an RMT TX channel is present between transmissions");
        let transaction = match channel.transmit(data) {
            Ok(transaction) => transaction,
            Err((error, channel)) => {
                self.channel = Some(channel);
                return Err(error);
            }
        };
        match transaction.wait() {
            Ok(channel) => {
                self.channel = Some(channel);
                Ok(())
            }
            Err((error, channel)) => {
                self.channel = Some(channel);
                Err(error)
            }
        }
    }
}

/// A configured RMT receive channel.
///
/// Same ownership story as [`RmtTxChannel`]: blocking transactions hand the
/// channel back, async ones borrow it.
pub struct RmtRxChannel<Dm: DriverMode> {
    channel: Option<Channel<'static, Dm, Rx>>,
}

impl<Dm: DriverMode> RmtRxChannel<Dm> {
    /// Wrap an already configured channel.
    pub fn new(channel: Channel<'static, Dm, Rx>) -> Self {
        Self {
            channel: Some(channel),
        }
    }
}

impl RmtRxChannel<esp_hal::Blocking> {
    /// Receive one pulse-code sequence into `buffer`, returning its length.
    pub fn receive(&mut self, buffer: &mut [PulseCode]) -> Result<usize, RmtError> {
        let channel = self
            .channel
            .take()
            .expect("an RMT RX channel is present between receptions");
        let transaction = match channel.receive(buffer) {
            Ok(transaction) => transaction,
            Err((error, channel)) => {
                self.channel = Some(channel);
                return Err(error);
            }
        };
        match transaction.wait() {
            Ok((length, channel)) => {
                self.channel = Some(channel);
                Ok(length)
            }
            Err((error, channel)) => {
                self.channel = Some(channel);
                Err(error)
            }
        }
    }
}

/// The blocking TX endpoint, as every `<rmt_tx>` and manifest names it.
pub type BlockingRmtTxEndpoint = RmtTxChannel<esp_hal::Blocking>;
/// The blocking RX endpoint, as every `<rmt_rx>` and manifest names it.
pub type BlockingRmtRxEndpoint = RmtRxChannel<esp_hal::Blocking>;

#[cfg(feature = "embassy")]
impl RmtTxChannel<esp_hal::Async> {
    /// Transmit one pulse-code sequence and await its completion.
    pub async fn transmit(&mut self, data: &[PulseCode]) -> Result<(), RmtError> {
        self.channel
            .as_mut()
            .expect("an RMT TX channel is present between transmissions")
            .transmit(data)
            .await
    }
}

#[cfg(feature = "embassy")]
impl RmtRxChannel<esp_hal::Async> {
    /// Receive one pulse-code sequence into `buffer`, awaiting completion.
    pub async fn receive(&mut self, buffer: &mut [PulseCode]) -> Result<usize, RmtError> {
        self.channel
            .as_mut()
            .expect("an RMT RX channel is present between receptions")
            .receive(buffer)
            .await
    }
}

/// The async TX endpoint, used by `<rmt_tx>` on an embassy project.
#[cfg(feature = "embassy")]
pub type AsyncRmtTxEndpoint = RmtTxChannel<esp_hal::Async>;
/// The async RX endpoint, used by `<rmt_rx>` on an embassy project.
#[cfg(feature = "embassy")]
pub type AsyncRmtRxEndpoint = RmtRxChannel<esp_hal::Async>;

/// Compile-time probe of the blocking TX/RX contract.
///
/// Never called: the target check type-checks it, which is the point — an
/// empty module would let a wrong signature reach M2 unnoticed.
#[allow(dead_code)]
pub fn _probe_tx_blocking(
    peripheral: esp_hal::peripherals::RMT<'static>,
    pin: impl esp_hal::gpio::interconnect::PeripheralOutput<'static>,
    data: &[PulseCode],
) -> Result<(), RmtError> {
    use esp_hal::rmt::{Rmt, TxChannelCreator};

    let rmt = Rmt::new(peripheral, Rate::from_mhz(80)).expect("probe: RMT clock");
    let channel = rmt
        .channel0
        .configure_tx(&TxChannelConfig::default())
        .expect("probe: TX channel")
        .with_pin(pin);
    let _channel = channel
        .transmit(data)
        .expect("probe: transmit")
        .wait()
        .expect("probe: wait");
    Ok(())
}

/// Compile-time probe of the async TX/RX contract.
///
/// Exercises the borrowed `&mut self` transmit/receive futures and the
/// whole-block `into_async`, so M2 does not have to guess either.
#[cfg(feature = "embassy")]
#[allow(dead_code)]
pub async fn _probe_tx_rx_async(
    peripheral: esp_hal::peripherals::RMT<'static>,
    tx_pin: impl esp_hal::gpio::interconnect::PeripheralOutput<'static>,
    rx_pin: impl esp_hal::gpio::interconnect::PeripheralInput<'static>,
    data: &[PulseCode],
    buffer: &mut [PulseCode],
) -> Result<usize, RmtError> {
    use esp_hal::rmt::{Rmt, RxChannelCreator, TxChannelCreator};

    let rmt = Rmt::new(peripheral, Rate::from_mhz(80))
        .expect("probe: RMT clock")
        .into_async();
    let mut tx = rmt
        .channel0
        .configure_tx(&TxChannelConfig::default())
        .expect("probe: TX channel")
        .with_pin(tx_pin);
    tx.transmit(data).await?;
    let mut rx = rmt
        .channel2
        .configure_rx(&RxChannelConfig::default())
        .expect("probe: RX channel")
        .with_pin(rx_pin);
    rx.receive(buffer).await
}

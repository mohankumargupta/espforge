//! `rc522` device: MFRC522 RFID reader over SPI (blocking).
//!
//! Phase 1 + Phase 2 + Phase 3 + Phase 4 + Phase 5 — SPI transport plus reset and defaults
//! (Phase 1), single-shot UID poll returning a hyphen-separated UID string
//! (Phase 2), configured-UID presence match turning a sensor on/off with
//! tag in/out (Phase 3), found/removed events firing once per tap-in and
//! tap-out (Phase 4), and the timed re-scan loop running the 1 s poll
//! unattended while logging unknown tags (Phase 5). Own-core port of
//! `esphome/components/rc522` + `rc522_spi`; no external driver crate.
//! Done when: firmware version reads back and `init` completes without error
//! (Phase 1); presenting tag returns hyphen-separated UID string (Phase 2);
//! configured UID sensor turns on/off with tag in/out (Phase 3); callbacks
//! fire once per tap-in and tap-out (Phase 4); 1 s poll runs unattended and
//! logs unknown tags (Phase 5).
//!
//! Register addresses are the pre-shifted values from ESPHome (`reg << 1`,
//! LSB reserved; datasheet §8.1.2.3): reads send `0x80 | reg`, writes send
//! `reg` as the address byte.
//!
//! Generic over `embedded-hal` traits (`SPI: SpiDevice`, `RST: OutputPin`,
//! `DLY: DelayNs`) so host unit tests can substitute `embedded-hal-mock`
//! types. Held behind `RefCell`s so every method takes `&self` (ADR-008
//! shared `&` context; mirrors `components/spi.rs:242-267`). Production
//! firmware uses the concrete [`EspRc522`] alias (no codegen change beyond
//! the type name).

use core::cell::RefCell;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::{Operation, SpiDevice as SpiDeviceTrait};

// Target builds keep the shared error so the public API is unchanged.
// (`cargo test` also builds the plain lib, so a `not(test)` gate would still
// name `esp-hal` on host — the gate must be on target arch, not test.)
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
use crate::components::spi::SpiError;

// Host builds cannot pull `crate::components::spi` — it wraps `esp-hal`
// types, which only exist on riscv32/xtensa (see `Cargo.toml`
// `[target.'cfg(...)'.dependencies]`). Same name and variant so test
// assertions read identically to production code.
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpiError {
    Bus,
}

#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
impl embedded_hal::spi::Error for SpiError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

// Page 0: command and status.
const COMMAND_REG: u8 = 0x01 << 1;
const COM_IRQ_REG: u8 = 0x04 << 1;
const DIV_IRQ_REG: u8 = 0x05 << 1;
const ERROR_REG: u8 = 0x06 << 1;
const FIFO_DATA_REG: u8 = 0x09 << 1;
const FIFO_LEVEL_REG: u8 = 0x0A << 1;
const CONTROL_REG: u8 = 0x0C << 1;
const BIT_FRAMING_REG: u8 = 0x0D << 1;
const COLL_REG: u8 = 0x0E << 1;
// Page 1: command.
const MODE_REG: u8 = 0x11 << 1;
const TX_MODE_REG: u8 = 0x12 << 1;
const RX_MODE_REG: u8 = 0x13 << 1;
const TX_CONTROL_REG: u8 = 0x14 << 1;
const TX_ASK_REG: u8 = 0x15 << 1;
// Page 2: configuration.
const CRC_RESULT_REG_H: u8 = 0x21 << 1;
const CRC_RESULT_REG_L: u8 = 0x22 << 1;
// Page 2: configuration.
const MOD_WIDTH_REG: u8 = 0x24 << 1;
const T_MODE_REG: u8 = 0x2A << 1;
const T_PRESCALER_REG: u8 = 0x2B << 1;
const T_RELOAD_REG_H: u8 = 0x2C << 1;
const T_RELOAD_REG_L: u8 = 0x2D << 1;
// Page 3: version.
const VERSION_REG: u8 = 0x37 << 1;

const PCD_SOFT_RESET: u8 = 0x0F;

// MFRC522 PCD commands (datasheet ch. 10) needed for Phase 2.
const PCD_IDLE: u8 = 0x00;
const PCD_CALC_CRC: u8 = 0x03;
const PCD_TRANSCEIVE: u8 = 0x0C;

// PICC commands (ISO 14443-3 Type A) needed for single-shot UID poll.
const PICC_CMD_REQA: u8 = 0x26;
const PICC_CMD_SEL_CL1: u8 = 0x93;
const PICC_CMD_SEL_CL2: u8 = 0x95;
const PICC_CMD_SEL_CL3: u8 = 0x97;
const PICC_CMD_CT: u8 = 0x88;

/// Phase-2 error: transport plus poll outcomes.
///
/// Phase-1 methods keep returning [`SpiError`] unchanged. Poll methods return
/// this so `Timeout` (no tag in field, caller maps to `Ok(None)`) stays
/// distinct from `Bus` (transport) and `Protocol` (unexpected response).
/// Same shape on host and target (target's `SpiError` is the shared
/// component error, which cannot gain poll variants).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PollError {
    Bus,
    /// Timer expired with nothing received — no tag in field.
    Timeout,
    /// Unexpected response (bad length, BCC mismatch, error-register bits,
    /// collision, short caller buffer).
    Protocol,
}

impl From<SpiError> for PollError {
    fn from(_: SpiError) -> Self {
        PollError::Bus
    }
}

/// Fixed re-scan interval in milliseconds (outline Essential #2: poll for a
/// nearby tag on a fixed 1 s scan interval, mirroring ESPHome's
/// `polling_component_schema("1s")` in `__init__.py`).
pub const SCAN_INTERVAL_MS: u32 = 1000;

/// Largest hyphen-separated UID text [`Rc522::format_uid`] can produce: a
/// 10-byte UID renders as 10 hex pairs joined by 9 `-` separators
/// (29 bytes). Phase-5 log buffers are sized with this.
pub const UID_TEXT_MAX: usize = 29;

/// MFRC522 reader on a shared SPI bus.
///
/// `spi` is the per-device handle; `reset` is the optional RST pin moved in
/// by value (`None` when unconnected, mirroring ESPHome's optional
/// `reset_pin`). Any underlying SPI/pin error maps to [`SpiError::Bus`]
/// in Phase-1 methods and [`PollError::Bus`] in Phase-2 poll methods.
pub struct Rc522<SPI, RST, DLY> {
    spi: RefCell<SPI>,
    reset: RefCell<Option<RST>>,
    delay: RefCell<DLY>,
}

/// Production instantiation for generated firmware (concrete `esp-hal`
/// types). Emitted by `espforge-bindings` as the device field type and ctor
/// target so `None` reset and type inference keep working.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub type EspRc522 = Rc522<
    crate::components::spi::SpiDevice<esp_hal::Blocking>,
    esp_hal::gpio::Output<'static>,
    crate::Delay,
>;

impl<SPI, RST, DLY> Rc522<SPI, RST, DLY>
where
    SPI: SpiDeviceTrait,
    RST: OutputPin,
    DLY: DelayNs,
{
    pub fn new(spi: SPI, reset: Option<RST>, delay: DLY) -> Self {
        Rc522 {
            spi: RefCell::new(spi),
            reset: RefCell::new(reset),
            delay: RefCell::new(delay),
        }
    }

    /// Read one register (datasheet §8.1.2.3: address byte `0x80 | reg`,
    /// then clock out the value).
    pub fn read_reg(&self, reg: u8) -> Result<u8, SpiError> {
        let mut buf = [0x80 | reg, 0];
        self.spi
            .borrow_mut()
            .transfer_in_place(&mut buf)
            .map_err(|_| SpiError::Bus)?;
        Ok(buf[1])
    }

    /// Write one register (address byte `reg`, then the value, CS held).
    pub fn write_reg(&self, reg: u8, value: u8) -> Result<(), SpiError> {
        self.spi
            .borrow_mut()
            .write(&[reg, value])
            .map_err(|_| SpiError::Bus)
    }

    /// Write `values` to `reg` (used for the FIFO). One transaction, CS held
    /// across the address byte and all payload bytes.
    pub fn write_regs(&self, reg: u8, values: &[u8]) -> Result<(), SpiError> {
        self.spi
            .borrow_mut()
            .transaction(&mut [Operation::Write(&[reg]), Operation::Write(values)])
            .map_err(|_| SpiError::Bus)
    }

    /// Read `values.len()` bytes from `reg` (used for the FIFO). Mirrors
    /// `RC522Spi::pcd_read_register` (esphome `rc522_spi.cpp`): the address
    /// byte is re-sent for every byte except the last, which sends `0x00`
    /// to stop reading. A single full-duplex transfer keeps CS held, so
    /// `rx[0]` is junk and `values[i] = rx[i + 1]`.
    ///
    /// `rx_align` (0..=7) merges only bits `rx_align..7` into `values[0]`,
    /// as in the original.
    pub fn read_regs(&self, reg: u8, values: &mut [u8], rx_align: u8) -> Result<(), SpiError> {
        if values.is_empty() {
            return Ok(());
        }
        // FIFO is 64 bytes; one extra byte carries the leading address.
        if values.len() > 64 {
            return Err(SpiError::Bus);
        }
        let addr = 0x80 | reg;
        let mut buf = [0u8; 65];
        buf[0] = addr;
        buf[1..values.len()].fill(addr);
        // Last byte sends 0x00 to stop reading.
        buf[values.len()] = 0;
        self.spi
            .borrow_mut()
            .transfer_in_place(&mut buf[..values.len() + 1])
            .map_err(|_| SpiError::Bus)?;
        if rx_align == 0 {
            values.copy_from_slice(&buf[1..values.len() + 1]);
        } else {
            let mask = (0xFFu8).wrapping_shl(rx_align as u32);
            values.copy_from_slice(&buf[1..values.len() + 1]);
            values[0] = (values[0] & !mask) | (buf[1] & mask);
        }
        Ok(())
    }

    /// Read the firmware version register (`VERSION_REG`).
    pub fn version(&self) -> Result<u8, SpiError> {
        self.read_reg(VERSION_REG)
    }

    /// Soft reset: issue `PCD_SOFT_RESET`, then poll the PowerDown bit
    /// (`COMMAND_REG` bit 4) until it clears. Retries mirror ESPHome's
    /// `pcd_reset_` (up to 3 × 50 ms); still set means the chip is down.
    pub fn soft_reset(&self) -> Result<(), SpiError> {
        self.write_reg(COMMAND_REG, PCD_SOFT_RESET)?;
        self.delay.borrow_mut().delay_ms(50);
        for _ in 0..3 {
            if self.read_reg(COMMAND_REG)? & (1 << 4) == 0 {
                return Ok(());
            }
            self.delay.borrow_mut().delay_ms(50);
        }
        Err(SpiError::Bus)
    }

    /// Phase-1 init: hard reset via the RST pin when present (2 µs low,
    /// then 50 ms oscillator start-up per datasheet §8.8.2), soft reset,
    /// then the default register set from ESPHome's `initialize_`.
    pub fn init(&self) -> Result<(), SpiError> {
        if let Some(rst) = self.reset.borrow_mut().as_mut() {
            rst.set_low().map_err(|_| SpiError::Bus)?;
            self.delay.borrow_mut().delay_ns(2_000);
            rst.set_high().map_err(|_| SpiError::Bus)?;
            self.delay.borrow_mut().delay_ms(50);
        }
        self.soft_reset()?;
        // Reset baud rates.
        self.write_reg(TX_MODE_REG, 0x00)?;
        self.write_reg(RX_MODE_REG, 0x00)?;
        // Reset ModWidth.
        self.write_reg(MOD_WIDTH_REG, 0x26)?;
        // Timer: auto-start, 40 kHz (25 µs period), 25 ms timeout.
        self.write_reg(T_MODE_REG, 0x80)?;
        self.write_reg(T_PRESCALER_REG, 0xA9)?;
        self.write_reg(T_RELOAD_REG_H, 0x03)?;
        self.write_reg(T_RELOAD_REG_L, 0xE8)?;
        // Force 100 % ASK modulation.
        self.write_reg(TX_ASK_REG, 0x40)?;
        // CRC preset 0x6363 (ISO 14443-3 §6.2.4).
        self.write_reg(MODE_REG, 0x3D)?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // Phase 2 — single-shot UID poll (blocking).
    //
    // Own-core port of the ESPHome `loop()` UID path (`rc522.cpp`:
    // `update` REQA + `STATE_READ_SERIAL`/`STATE_SELECT_SERIAL` cascade),
    // collapsed to blocking calls: `pcd_transceive_data_` +
    // `await_transceive_` become `transceive`, `pcd_calculate_crc_` +
    // `await_crc_` become `calculate_crc`. Antenna is powered around each
    // scan cycle (outline Essential #8).
    // ---------------------------------------------------------------

    fn set_bit_mask(&self, reg: u8, mask: u8) -> Result<(), PollError> {
        let tmp = self.read_reg(reg)?;
        self.write_reg(reg, tmp | mask)?;
        Ok(())
    }

    fn clear_bit_mask(&self, reg: u8, mask: u8) -> Result<(), PollError> {
        let tmp = self.read_reg(reg)?;
        self.write_reg(reg, tmp & !mask)?;
        Ok(())
    }

    /// Turn the antenna on (enable TX1/TX2 drivers).
    pub fn antenna_on(&self) -> Result<(), PollError> {
        let value = self.read_reg(TX_CONTROL_REG)?;
        if value & 0x03 != 0x03 {
            self.write_reg(TX_CONTROL_REG, value | 0x03)?;
        }
        Ok(())
    }

    /// Turn the antenna off (disable TX1/TX2 drivers).
    pub fn antenna_off(&self) -> Result<(), PollError> {
        let value = self.read_reg(TX_CONTROL_REG)?;
        if value & 0x03 != 0x00 {
            self.write_reg(TX_CONTROL_REG, value & !0x03)?;
        }
        Ok(())
    }

    /// CRC-A over `data` using the on-chip coprocessor (blocking).
    ///
    /// Mirrors `pcd_calculate_crc_` + `await_crc_`: Idle, clear CRCIRq,
    /// flush FIFO, write payload, start calculation, then poll `DIV_IRQ_REG`
    /// bit 2 up to ~89 ms. Returns `[CRC_L, CRC_H]` (transmit order).
    fn calculate_crc(&self, data: &[u8]) -> Result<[u8; 2], PollError> {
        if data.is_empty() || data.len() > 64 {
            return Err(PollError::Protocol);
        }
        self.write_reg(COMMAND_REG, PCD_IDLE)?;
        self.write_reg(DIV_IRQ_REG, 0x04)?;
        self.write_reg(FIFO_LEVEL_REG, 0x80)?;
        self.write_regs(FIFO_DATA_REG, data)?;
        self.write_reg(COMMAND_REG, PCD_CALC_CRC)?;
        for _ in 0..89 {
            let n = self.read_reg(DIV_IRQ_REG)?;
            if n & 0x04 != 0 {
                self.write_reg(COMMAND_REG, PCD_IDLE)?;
                let lo = self.read_reg(CRC_RESULT_REG_L)?;
                let hi = self.read_reg(CRC_RESULT_REG_H)?;
                return Ok([lo, hi]);
            }
            self.delay.borrow_mut().delay_ms(1);
        }
        Err(PollError::Timeout)
    }

    /// Transceive `send` to the tag and read the reply into `recv` (blocking).
    ///
    /// Mirrors `pcd_transceive_data_` + `await_transceive_`: 1 ms antenna
    /// settle, Idle, clear interrupts, flush FIFO, write payload, set
    /// `BitFramingReg` to `tx_last_bits`, start `PCD_TRANSCEIVE`, then poll
    /// `COM_IRQ_REG` up to ~40 ms. Timer expiry is `Timeout` (no tag);
    /// `ERROR_REG & 0x13`, short buffers, partial last byte, or collision
    /// (`CollErr`) are `Protocol`. Returns bytes placed in `recv`.
    fn transceive(
        &self,
        send: &[u8],
        tx_last_bits: u8,
        recv: &mut [u8],
    ) -> Result<usize, PollError> {
        if send.is_empty() || send.len() > 64 || recv.is_empty() {
            return Err(PollError::Protocol);
        }
        self.delay.borrow_mut().delay_us(1000);
        self.write_reg(COMMAND_REG, PCD_IDLE)?;
        self.write_reg(COM_IRQ_REG, 0x7F)?;
        self.write_reg(FIFO_LEVEL_REG, 0x80)?;
        self.write_regs(FIFO_DATA_REG, send)?;
        self.write_reg(BIT_FRAMING_REG, tx_last_bits & 0x07)?;
        self.write_reg(COMMAND_REG, PCD_TRANSCEIVE)?;
        self.set_bit_mask(BIT_FRAMING_REG, 0x80)?;
        for _ in 0..40 {
            // ESPHome waits at least 2 ms before sampling the IRQ register.
            self.delay.borrow_mut().delay_ms(1);
            let n = self.read_reg(COM_IRQ_REG)?;
            if n & 0x01 != 0 {
                return Err(PollError::Timeout);
            }
            if n & 0x30 == 0 {
                continue;
            }
            let err = self.read_reg(ERROR_REG)?;
            if err & 0x13 != 0 {
                return Err(PollError::Protocol);
            }
            let level = self.read_reg(FIFO_LEVEL_REG)? as usize;
            if level == 0 || level > recv.len() || level > 64 {
                return Err(PollError::Protocol);
            }
            self.read_regs(FIFO_DATA_REG, &mut recv[..level], 0)?;
            let valid = self.read_reg(CONTROL_REG)? & 0x07;
            if err & 0x08 != 0 {
                return Err(PollError::Protocol);
            }
            if valid != 0 {
                return Err(PollError::Protocol);
            }
            return Ok(level);
        }
        Err(PollError::Timeout)
    }

    /// REQA probe: `Ok(true)` + ATQA when a tag answers, `Ok(false)` when
    /// the field is empty (timer expiry). Antenna must already be on.
    fn request_a(&self, atqa: &mut [u8; 2]) -> Result<bool, PollError> {
        let mut rx = [0u8; 2];
        match self.transceive(&[PICC_CMD_REQA], 7, &mut rx) {
            Ok(2) => {
                atqa.copy_from_slice(&rx);
                Ok(true)
            }
            Ok(_) => Err(PollError::Protocol),
            Err(PollError::Timeout) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Anticollision + select for one cascade level.
    ///
    /// Sends `SEL, 0x20`, checks the 5-byte anticollision reply (4 UID bytes
    /// plus BCC xor), then selects with CRC and reads the 3-byte SAK.
    /// On success appends the level's UID bytes (skipping cascade tag
    /// `0x88`) to `uid` at `uid_idx`, reporting `(advanced_len, more)`.
    fn select_cascade_level(
        &self,
        sel: u8,
        uid: &mut [u8; 10],
        uid_idx: usize,
    ) -> Result<(usize, bool), PollError> {
        let mut anti = [0u8; 5];
        let n = self.transceive(&[sel, 0x20], 0, &mut anti)?;
        if n != 5 {
            return Err(PollError::Protocol);
        }
        if anti[0] ^ anti[1] ^ anti[2] ^ anti[3] != anti[4] {
            return Err(PollError::Protocol);
        }
        let cascade = anti[0] == PICC_CMD_CT;
        let copy_start = if cascade { 1 } else { 0 };
        let copy_len = 4 - copy_start;
        if uid_idx + copy_len > uid.len() {
            return Err(PollError::Protocol);
        }
        uid[uid_idx..uid_idx + copy_len].copy_from_slice(&anti[copy_start..4]);
        // SELECT: SEL, 0x70, 4 UID bytes, BCC, CRC_L, CRC_H (9 bytes).
        let mut frame = [0u8; 7];
        frame[0] = sel;
        frame[1] = 0x70;
        frame[2..6].copy_from_slice(&anti[..4]);
        frame[6] = anti[4];
        let crc = self.calculate_crc(&frame)?;
        let mut select = [0u8; 9];
        select[..7].copy_from_slice(&frame);
        select[7] = crc[0];
        select[8] = crc[1];
        let mut sak = [0u8; 3];
        let m = self.transceive(&select, 0, &mut sak)?;
        if m != 3 {
            return Err(PollError::Protocol);
        }
        Ok((uid_idx + copy_len, sak[0] & 0x04 != 0))
    }

    /// Single-shot UID poll (blocking).
    ///
    /// Powers the antenna around the scan (Essential #8), probes with REQA,
    /// then walks cascade levels 1–3. Fills `uid` and returns `Some(len)`
    /// (`len` 4, 7, or 10) when a tag is present, `None` when the field is
    /// empty. Pair with [`Self::format_uid`] for the hyphen-separated string.
    pub fn poll_uid(&self, uid: &mut [u8; 10]) -> Result<Option<usize>, PollError> {
        self.antenna_on()?;
        self.clear_bit_mask(COLL_REG, 0x80)?;
        let found = self.poll_uid_inner(uid);
        // Antenna power is managed around each scan cycle: always attempt to
        // power down before returning, preserving the poll outcome.
        let off = self.antenna_off();
        match (found, off) {
            (r, Ok(())) => r,
            (Err(e), Err(_)) => Err(e),
            (r, Err(e)) => {
                let _ = r?;
                Err(e)
            }
        }
    }

    fn poll_uid_inner(&self, uid: &mut [u8; 10]) -> Result<Option<usize>, PollError> {
        let mut atqa = [0u8; 2];
        if !self.request_a(&mut atqa)? {
            return Ok(None);
        }
        if atqa.len() != 2 {
            return Err(PollError::Protocol);
        }
        let mut idx = 0usize;
        for &sel in &[PICC_CMD_SEL_CL1, PICC_CMD_SEL_CL2, PICC_CMD_SEL_CL3] {
            let (next, more) = self.select_cascade_level(sel, uid, idx)?;
            idx = next;
            if !more {
                return Ok(Some(idx));
            }
            if idx >= uid.len() {
                return Err(PollError::Protocol);
            }
        }
        Err(PollError::Protocol)
    }

    /// Format `uid` as uppercase hex pairs joined by `-` (ESPHome
    /// `format_hex_pretty_to(..., '-')`, e.g. `74-10-37-94`) into `out`.
    /// Returns bytes written; stops at the last whole byte that fits so the
    /// result never ends with a dangling separator.
    pub fn format_uid(uid: &[u8], out: &mut [u8]) -> usize {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut pos = 0usize;
        for (i, &b) in uid.iter().enumerate() {
            let need = if i == 0 { 2 } else { 3 };
            if out.len().saturating_sub(pos) < need {
                break;
            }
            if i != 0 {
                out[pos] = b'-';
                pos += 1;
            }
            out[pos] = HEX[(b >> 4) as usize];
            out[pos + 1] = HEX[(b & 0x0F) as usize];
            pos += 2;
        }
        pos
    }

    /// Poll then format in one call: fills `uid_bin`, writes the
    /// hyphen-separated UID string into `out`, returns `Some(str_len)` on a
    /// tag, `None` when empty. `Protocol` when the UID string would not fit.
    pub fn poll_uid_str(
        &self,
        uid_bin: &mut [u8; 10],
        out: &mut [u8],
    ) -> Result<Option<usize>, PollError> {
        match self.poll_uid(uid_bin)? {
            None => Ok(None),
            Some(n) => {
                let written = Self::format_uid(&uid_bin[..n], out);
                let need = if n == 0 { 0 } else { n * 3 - 1 };
                if written != need {
                    return Err(PollError::Protocol);
                }
                Ok(Some(written))
            }
        }
    }

    /// One scan folded into `sensor` (blocking).
    ///
    /// Wires [`Self::poll_uid`] to the sensor the way ESPHome's `loop()`
    /// wires the cascade result to its binary sensors
    /// (`STATE_READ_SERIAL_DONE` runs `process`, the `STATE_PICC_REQUEST_A`
    /// timeout runs `on_scan_end`): a UID runs [`TagPresence::process`], an
    /// empty field runs [`TagPresence::on_scan_end`]. Returns the sensor's
    /// new on/off state. Transport/protocol errors propagate with the sensor
    /// untouched.
    pub fn poll_presence(
        &self,
        sensor: &mut TagPresence,
        uid: &mut [u8; 10],
    ) -> Result<bool, PollError> {
        match self.poll_uid(uid)? {
            Some(n) => Ok(sensor.process(&uid[..n])),
            None => Ok(sensor.on_scan_end()),
        }
    }

    /// One scan folded into `watcher` (blocking).
    ///
    /// Wires [`Self::poll_uid`] to the tap-in/tap-out tracker the way
    /// ESPHome's `loop()` wires the cascade result to its triggers
    /// (`STATE_READ_SERIAL_DONE` runs `triggers_ontag_` when
    /// `current_uid_ != rfid_uid`, `STATE_DONE` runs
    /// `triggers_ontagremoved_` when the field goes empty): a UID runs
    /// [`TagWatcher::process`], an empty field runs
    /// [`TagWatcher::on_scan_end`]. Returns the transition. Transport/
    /// protocol errors propagate with the watcher untouched.
    pub fn poll_watcher(
        &self,
        watcher: &mut TagWatcher,
        uid: &mut [u8; 10],
    ) -> Result<TagEvent, PollError> {
        match self.poll_uid(uid)? {
            Some(n) => Ok(watcher.process(&uid[..n])),
            None => Ok(watcher.on_scan_end()),
        }
    }

    /// One scan folded into `watcher` with callbacks fired once per
    /// transition (blocking).
    ///
    /// Same scan as [`Self::poll_watcher`], then invokes `on_found` once
    /// when a new UID taps in and `on_removed` once when the UID taps out;
    /// steady state (same tag still present, field still empty) and errors
    /// invoke neither callback. Callbacks receive the UID bytes (format
    /// with [`Self::format_uid`] for the hyphen-separated string). Returns
    /// the transition.
    pub fn poll_tag_events(
        &self,
        watcher: &mut TagWatcher,
        uid: &mut [u8; 10],
        mut on_found: impl FnMut(&[u8]),
        mut on_removed: impl FnMut(&[u8]),
    ) -> Result<TagEvent, PollError> {
        let event = self.poll_watcher(watcher, uid)?;
        match event {
            TagEvent::Found => {
                if let Some(current) = watcher.current_uid() {
                    on_found(current);
                }
            }
            TagEvent::Removed => {
                on_removed(watcher.removed_uid());
            }
            TagEvent::None => {}
        }
        Ok(event)
    }

    // ---------------------------------------------------------------
    // Phase 5 — timed re-scan loop (blocking).
    //
    // Own-core port of ESPHome's 1 s poll tick (`polling_component_schema(
    // "1s")` in `__init__.py`: `update()` powers the antenna and starts a
    // scan) plus the `STATE_READ_SERIAL_DONE` report branch (an unmatched
    // UID logs `ESP_LOGD(TAG, "Found new tag '%s'")` for enrollment
    // copy-paste) and the `STATE_DONE` removal branch. `poll_cycle` is one
    // cycle; `run_scan_loop` runs cycles unattended with the fixed
    // `SCAN_INTERVAL_MS` delay between them.
    // ---------------------------------------------------------------

    /// One timed scan cycle: presence, events, and unknown-tag logging
    /// (blocking).
    ///
    /// Ports one ESPHome `update()` tick plus the resulting `loop()` scan
    /// (`rc522.cpp`): polls the UID, folds it into every presence `sensor`
    /// (the `binary_sensors_` loop — a match anywhere marks the tag known),
    /// folds it into `watcher`, fires `on_found` once per tap-in and
    /// `on_removed` once per tap-out, and logs unknown UIDs (outline
    /// Essential #7) by formatting the hyphen-separated UID string into
    /// `uid_text` (sized [`UID_TEXT_MAX`], always big enough for any UID
    /// [`Self::poll_uid`] yields) and emitting it via `log`. Steady-state
    /// re-scans, known tags, and empty-field scans leave `uid_text`
    /// untouched. Transport/protocol errors propagate with sensors,
    /// watcher, and `uid_text` all untouched. Returns the tap-in/tap-out
    /// transition.
    pub fn poll_cycle(
        &self,
        watcher: &mut TagWatcher,
        sensors: &mut [&mut TagPresence],
        uid: &mut [u8; 10],
        uid_text: &mut [u8; UID_TEXT_MAX],
        mut on_found: impl FnMut(&[u8]),
        mut on_removed: impl FnMut(&[u8]),
    ) -> Result<TagEvent, PollError> {
        match self.poll_uid(uid)? {
            Some(n) => {
                let scanned = &uid[..n];
                let mut known = false;
                for sensor in sensors.iter_mut() {
                    if sensor.process(scanned) {
                        known = true;
                    }
                }
                let event = watcher.process(scanned);
                match event {
                    TagEvent::Found => {
                        if let Some(current) = watcher.current_uid() {
                            on_found(current);
                            if !known {
                                let written = Self::format_uid(current, uid_text);
                                if let Ok(text) = core::str::from_utf8(&uid_text[..written]) {
                                    log::debug!("rc522: Found new tag '{}'", text);
                                }
                            }
                        }
                    }
                    TagEvent::Removed => {
                        on_removed(watcher.removed_uid());
                    }
                    TagEvent::None => {}
                }
                Ok(event)
            }
            None => {
                for sensor in sensors.iter_mut() {
                    sensor.on_scan_end();
                }
                let event = watcher.on_scan_end();
                if event == TagEvent::Removed {
                    on_removed(watcher.removed_uid());
                }
                Ok(event)
            }
        }
    }

    /// Timed re-scan loop: runs [`Self::poll_cycle`] every
    /// [`SCAN_INTERVAL_MS`] milliseconds, forever (blocking).
    ///
    /// This is the unattended 1 s poll (outline Essential #2): per-cycle
    /// scan errors are dropped so the loop keeps running, matching ESPHome's
    /// `PollingComponent` tick which re-scans on the next interval after a
    /// failed cycle.
    pub fn run_scan_loop(
        &self,
        watcher: &mut TagWatcher,
        sensors: &mut [&mut TagPresence],
        uid: &mut [u8; 10],
        uid_text: &mut [u8; UID_TEXT_MAX],
        mut on_found: impl FnMut(&[u8]),
        mut on_removed: impl FnMut(&[u8]),
    ) -> ! {
        loop {
            let _ = self.poll_cycle(
                watcher,
                &mut *sensors,
                uid,
                uid_text,
                &mut on_found,
                &mut on_removed,
            );
            self.delay.borrow_mut().delay_ms(SCAN_INTERVAL_MS);
        }
    }
}

/// One tap-in/tap-out transition for any UID in the field.
///
/// Own-core port of ESPHome's `current_uid_` trigger logic (`rc522.cpp`:
/// `STATE_READ_SERIAL_DONE` fires `triggers_ontag_` when the scanned UID
/// differs from `current_uid_`, `STATE_DONE` fires
/// `triggers_ontagremoved_` when the field goes empty). `Found` carries the
/// newly tapped-in UID (see [`TagWatcher::current_uid`]), `Removed` carries
/// the just-removed UID (see [`TagWatcher::removed_uid`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TagEvent {
    /// Steady state: same tag still present, or field still empty.
    None,
    /// New UID tapped in (differs from the previously tracked UID).
    Found,
    /// Tracked UID left the field (empty scan after a presence).
    Removed,
}

/// Tracker for found/removed events across scans: remembers the UID
/// currently in the field so callbacks fire once per tap-in and tap-out.
///
/// Own-core port of ESPHome's `current_uid_` (`rc522.h` / `rc522.cpp`).
/// `no_std`-compatible: fixed `[u8; 10]` storage, no heap. Holds at most
/// one UID (multi-tag lists are Phase-5 Desirable #10, out of scope).
pub struct TagWatcher {
    current: [u8; 10],
    len: usize,
    present: bool,
    removed: [u8; 10],
    removed_len: usize,
}

impl TagWatcher {
    /// Fresh tracker: nothing in the field, no pending removed UID.
    pub fn new() -> Self {
        TagWatcher {
            current: [0u8; 10],
            len: 0,
            present: false,
            removed: [0u8; 10],
            removed_len: 0,
        }
    }

    /// UID currently in the field (`None` when the field is empty).
    pub fn current_uid(&self) -> Option<&[u8]> {
        if self.present {
            Some(&self.current[..self.len])
        } else {
            None
        }
    }

    /// UID of the most recent tap-out (valid after [`TagEvent::Removed`]
    /// until the next [`TagEvent::Found`]; empty before the first removal).
    pub fn removed_uid(&self) -> &[u8] {
        &self.removed[..self.removed_len]
    }

    /// Whether a tag is currently in the field.
    pub fn is_present(&self) -> bool {
        self.present
    }

    /// Fold one successful scan into the tracker (port of the
    /// `STATE_READ_SERIAL_DONE` branch, esphome `rc522.cpp`): a UID
    /// differing from the tracked one stores it and reports
    /// [`TagEvent::Found`]; the same UID re-scanned reports
    /// [`TagEvent::None`] (no retrigger). A direct swap to a different UID
    /// without an empty scan in between reports `Found` for the new UID
    /// only (no `Removed` for the old one, as in the original).
    /// Empty or over-long input reports `None` with state untouched.
    pub fn process(&mut self, scanned: &[u8]) -> TagEvent {
        if scanned.is_empty() || scanned.len() > self.current.len() {
            return TagEvent::None;
        }
        if self.present && TagPresence::uid_matches(&self.current[..self.len], scanned) {
            return TagEvent::None;
        }
        self.current[..scanned.len()].copy_from_slice(scanned);
        self.len = scanned.len();
        self.present = true;
        TagEvent::Found
    }

    /// Fold an empty scan into the tracker (port of the `STATE_DONE`
    /// branch, esphome `rc522.cpp`): a tracked UID reports
    /// [`TagEvent::Removed`] and is stashed in [`Self::removed_uid`]
    /// before clearing; an already-empty field reports [`TagEvent::None`].
    pub fn on_scan_end(&mut self) -> TagEvent {
        if !self.present {
            return TagEvent::None;
        }
        self.removed[..self.len].copy_from_slice(&self.current[..self.len]);
        self.removed_len = self.len;
        self.len = 0;
        self.present = false;
        TagEvent::Removed
    }

    /// Fold one scan outcome into the tracker: `Some(uid)` scans like
    /// [`Self::process`], `None` (field empty) like [`Self::on_scan_end`].
    /// Returns the transition.
    pub fn update(&mut self, scanned: Option<&[u8]>) -> TagEvent {
        match scanned {
            Some(uid) => self.process(uid),
            None => self.on_scan_end(),
        }
    }
}

impl Default for TagWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// One configured tag sensor: on/off presence for a single UID.
///
/// Own-core port of ESPHome's `RC522BinarySensor` (`rc522.h` / `rc522.cpp`:
/// `process` + `on_scan_end`). Holds the configured UID (4, 7, or 10 bytes —
/// the only sizes the cascade select yields), the published on/off state,
/// and whether this scan cycle matched. `no_std`-compatible: fixed
/// `[u8; 10]` storage, no heap.
pub struct TagPresence {
    uid: [u8; 10],
    len: usize,
    present: bool,
    found: bool,
}

impl TagPresence {
    /// Configure a sensor for `uid` (must be 4, 7, or 10 bytes — the only
    /// sizes [`Rc522::poll_uid`] yields). Returns `None` for any other
    /// length: such a UID can never match, so fail fast instead of tracking
    /// a sensor that stays off forever.
    pub fn new(uid: &[u8]) -> Option<Self> {
        if !matches!(uid.len(), 4 | 7 | 10) {
            return None;
        }
        let mut stored = [0u8; 10];
        stored[..uid.len()].copy_from_slice(uid);
        Some(TagPresence {
            uid: stored,
            len: uid.len(),
            present: false,
            found: false,
        })
    }

    /// Build from the hyphen-separated string [`Rc522::format_uid`]
    /// produces (e.g. `74-10-37-94`, the ESPHome `binary_sensor` `uid:`
    /// form, copied from the Phase-5 unknown-tag log). Returns `None` on
    /// malformed text or a non-4/7/10-byte UID.
    pub fn from_uid_str(text: &[u8]) -> Option<Self> {
        let mut raw = [0u8; 10];
        let n = Self::parse_uid(text, &mut raw)?;
        Self::new(&raw[..n])
    }

    /// Parse `74-10-37-94` into bytes. Each group must be exactly two hex
    /// digits (upper- or lowercase) joined by single `-` separators; writes
    /// 1..=10 groups into `out` and returns the byte count. Returns `None`
    /// on malformed input or overflow. Round-trips
    /// [`Rc522::format_uid`].
    pub fn parse_uid(text: &[u8], out: &mut [u8; 10]) -> Option<usize> {
        if text.is_empty() || text.len() > 29 {
            return None;
        }
        let mut n = 0usize;
        let mut i = 0usize;
        loop {
            // Each group is exactly two hex digits.
            if i + 2 > text.len() {
                return None;
            }
            let hi = Self::hex_val(text[i])?;
            let lo = Self::hex_val(text[i + 1])?;
            if n >= out.len() {
                return None;
            }
            out[n] = (hi << 4) | lo;
            n += 1;
            i += 2;
            if i == text.len() {
                return Some(n);
            }
            // Groups are joined by a single `-` (no leading/trailing/doubled).
            if text[i] != b'-' {
                return None;
            }
            i += 1;
            if i == text.len() {
                return None;
            }
        }
    }

    fn hex_val(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }

    /// Exact UID equality (the comparison inside `process`): same length
    /// and same bytes. A prefix never matches.
    pub fn uid_matches(a: &[u8], b: &[u8]) -> bool {
        a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
    }

    /// Configured UID bytes.
    pub fn uid(&self) -> &[u8] {
        &self.uid[..self.len]
    }

    /// Current on/off state (the published binary-sensor state).
    pub fn is_present(&self) -> bool {
        self.present
    }

    /// Fold one successful scan into the sensor (port of
    /// `RC522BinarySensor::process`, esphome `rc522.cpp`): an exact match
    /// turns the sensor on, anything else turns it off. Returns the new
    /// state.
    pub fn process(&mut self, scanned: &[u8]) -> bool {
        let matched = Self::uid_matches(self.uid(), scanned);
        self.present = matched;
        self.found = matched;
        matched
    }

    /// Fold an empty scan into the sensor (port of
    /// `RC522BinarySensor::on_scan_end`, esphome `rc522.cpp`): a sensor that
    /// matched earlier in this scan cycle stays on until the next empty
    /// scan, so presence lingers one extra scan after the tag leaves the
    /// field. Returns the new state.
    pub fn on_scan_end(&mut self) -> bool {
        if !self.found {
            self.present = false;
        }
        self.found = false;
        self.present
    }

    /// Fold one scan outcome into the sensor: `Some(uid)` scans like
    /// [`Self::process`], `None` (field empty) like [`Self::on_scan_end`].
    /// Returns the new state.
    pub fn update(&mut self, scanned: Option<&[u8]>) -> bool {
        match scanned {
            Some(uid) => self.process(uid),
            None => self.on_scan_end(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal_mock::eh1::delay::NoopDelay;
    use embedded_hal_mock::eh1::digital::{
        Mock as PinMock, State as PinState, Transaction as PinTx,
    };
    use embedded_hal_mock::eh1::spi::{Mock as SpiMock, Transaction as SpiTx};

    #[test]
    fn version_returns_firmware_revision() {
        // The mock's `SpiDevice::transaction` requires every call to be
        // wrapped in `transaction_start` / `transaction_end` (§14 risk 2:
        // adjust the expectation, not the driver).
        let expectations = [
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0xEE, 0x00], vec![0x00, 0x92]),
            SpiTx::transaction_end(),
        ];
        let mut spi = SpiMock::new(&expectations);
        // `Mock` shares expectations through `Arc`, so the clone moved into
        // the driver and this handle verify the same transaction log.
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.version(), Ok(0x92));
        spi.done();
    }

    /// One helper for all mock-SPI tests (§14 risk 1).
    fn spi_mock(expectations: &[SpiTx<u8>]) -> SpiMock<u8> {
        SpiMock::new(expectations)
    }

    /// `embedded-hal-mock` 0.11 spi `Mock` has no `with_error` (only the
    /// digital/i2c mocks do), so error mapping is tested with this fake.
    struct FailSpi;

    impl embedded_hal::spi::ErrorType for FailSpi {
        type Error = embedded_hal::spi::ErrorKind;
    }

    impl SpiDeviceTrait for FailSpi {
        fn transaction(
            &mut self,
            _operations: &mut [Operation<'_, u8>],
        ) -> Result<(), Self::Error> {
            Err(embedded_hal::spi::ErrorKind::Other)
        }
    }

    fn failing_reader() -> Rc522<FailSpi, PinMock, NoopDelay> {
        Rc522::new(FailSpi, None::<PinMock>, NoopDelay::new())
    }

    #[test]
    fn r1_read_reg_happy() {
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x82, 0x00], vec![0x00, 0xAB]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.read_reg(0x02), Ok(0xAB));
        spi.done();
    }

    #[test]
    fn r2_read_reg_error_propagates() {
        assert_eq!(failing_reader().read_reg(0x02), Err(SpiError::Bus));
    }

    #[test]
    fn w1_write_reg_happy() {
        // `SpiDevice::write` lowers to `transaction([Write])`.
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![0x24, 0x00]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.write_reg(0x24, 0x00), Ok(()));
        spi.done();
    }

    #[test]
    fn w2_write_reg_error_propagates() {
        assert_eq!(failing_reader().write_reg(0x24, 0x00), Err(SpiError::Bus));
    }

    #[test]
    fn wf1_write_regs_multi_byte_single_transaction() {
        // `0x09 << 1` is the FIFO address; both `Write` ops share one
        // transaction (CS held).
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![0x12]),
            SpiTx::write_vec(vec![1, 2, 3]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.write_regs(0x09 << 1, &[1, 2, 3]), Ok(()));
        spi.done();
    }

    #[test]
    fn wf2_write_regs_empty_slice_still_transacts() {
        // Pinned behavior: no special-case for empty payloads — the address
        // byte and an empty second write are still transacted.
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![0x12]),
            SpiTx::write_vec(vec![]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.write_regs(0x09 << 1, &[]), Ok(()));
        spi.done();
    }

    #[test]
    fn wf3_write_regs_error_propagates() {
        assert_eq!(
            failing_reader().write_regs(0x09 << 1, &[1, 2, 3]),
            Err(SpiError::Bus)
        );
    }

    #[test]
    fn rf1_read_regs_empty_is_noop_without_traffic() {
        let mut spi = spi_mock(&[]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values: [u8; 0] = [];
        assert_eq!(reader.read_regs(0x02, &mut values, 0), Ok(()));
        spi.done();
    }

    #[test]
    fn rf2_read_regs_single_byte() {
        // len 1 sends `[addr, 0x00]`; `rx[0]` is junk.
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x82, 0x00], vec![0xFF, 0x5A]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values = [0u8; 1];
        assert_eq!(reader.read_regs(0x02, &mut values, 0), Ok(()));
        assert_eq!(values, [0x5A]);
        spi.done();
    }

    #[test]
    fn rf3_read_regs_multi_byte_copies_payload() {
        // Address re-sent for every byte except the last (`0x00` stops).
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x84, 0x84, 0x84, 0x00], vec![0x00, 0x11, 0x22, 0x33]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values = [0u8; 3];
        assert_eq!(reader.read_regs(0x04, &mut values, 0), Ok(()));
        assert_eq!(values, [0x11, 0x22, 0x33]);
        spi.done();
    }

    #[test]
    fn rf4_read_regs_rx_align_merges_first_byte() {
        // `rx_align = 4` → `mask = 0xF0`; distinct nibbles pin the merge.
        // (Pinned as-is: the payload is copied before merging, so the merge
        // currently resolves to the received byte itself.)
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x84, 0x84, 0x84, 0x00], vec![0x00, 0xB4, 0x22, 0x33]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values = [0u8; 3];
        assert_eq!(reader.read_regs(0x04, &mut values, 4), Ok(()));
        assert_eq!(values, [0xB4, 0x22, 0x33]);
        spi.done();
    }

    #[test]
    fn rf5_read_regs_too_long_is_rejected_without_traffic() {
        let mut spi = spi_mock(&[]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values = [0u8; 65];
        assert_eq!(reader.read_regs(0x04, &mut values, 0), Err(SpiError::Bus));
        spi.done();
    }

    #[test]
    fn rf6_read_regs_error_propagates() {
        let mut values = [0u8; 2];
        assert_eq!(
            failing_reader().read_regs(0x04, &mut values, 0),
            Err(SpiError::Bus)
        );
    }

    /// `soft_reset` frame: `write_reg(COMMAND, 0x0F)` then polls.
    fn soft_reset_write() -> [SpiTx<u8>; 3] {
        [
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![0x02, 0x0F]),
            SpiTx::transaction_end(),
        ]
    }

    /// One `read_reg(COMMAND)` poll returning `value`.
    fn command_poll(value: u8) -> [SpiTx<u8>; 3] {
        [
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x82, 0x00], vec![0x00, value]),
            SpiTx::transaction_end(),
        ]
    }

    #[test]
    fn s1_soft_reset_immediate_clear() {
        let expectations = [soft_reset_write(), command_poll(0x00)].concat();
        let mut spi = spi_mock(&expectations);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.soft_reset(), Ok(()));
        spi.done();
    }

    #[test]
    fn s2_soft_reset_delayed_clear() {
        // Bit 4 set twice, then clear — all three polls consumed.
        let expectations = [
            soft_reset_write(),
            command_poll(0x10),
            command_poll(0x10),
            command_poll(0x00),
        ]
        .concat();
        let mut spi = spi_mock(&expectations);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.soft_reset(), Ok(()));
        spi.done();
    }

    #[test]
    fn s3_soft_reset_stuck_power_down_bit() {
        let expectations = [
            soft_reset_write(),
            command_poll(0x10),
            command_poll(0x10),
            command_poll(0x10),
        ]
        .concat();
        let mut spi = spi_mock(&expectations);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.soft_reset(), Err(SpiError::Bus));
        spi.done();
    }

    #[test]
    fn s4_soft_reset_write_failure_aborts() {
        // `FailSpi` fails the opening write, so no polls are attempted.
        assert_eq!(failing_reader().soft_reset(), Err(SpiError::Bus));
    }

    /// One `write_reg` frame for the `init` default register set.
    fn reg_write(reg: u8, value: u8) -> [SpiTx<u8>; 3] {
        [
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![reg, value]),
            SpiTx::transaction_end(),
        ]
    }

    /// Exact ESPHome `initialize_` register order (after `soft_reset`).
    fn init_defaults() -> Vec<SpiTx<u8>> {
        [
            reg_write(0x24, 0x00), // TX_MODE
            reg_write(0x26, 0x00), // RX_MODE
            reg_write(0x48, 0x26), // MOD_WIDTH
            reg_write(0x54, 0x80), // T_MODE
            reg_write(0x56, 0xA9), // T_PRESCALER
            reg_write(0x58, 0x03), // T_RELOAD_H
            reg_write(0x5A, 0xE8), // T_RELOAD_L
            reg_write(0x2A, 0x40), // TX_ASK
            reg_write(0x22, 0x3D), // MODE
        ]
        .concat()
    }

    fn init_soft_reset_ok() -> Vec<SpiTx<u8>> {
        [soft_reset_write(), command_poll(0x00)].concat()
    }

    #[test]
    fn i1_init_without_reset_pin_writes_defaults_in_order() {
        let mut expectations = init_soft_reset_ok();
        expectations.extend(init_defaults());
        let mut spi = spi_mock(&expectations);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.init(), Ok(()));
        spi.done();
    }

    #[test]
    fn i2_init_with_reset_pin_hard_resets_first() {
        let mut expectations = init_soft_reset_ok();
        expectations.extend(init_defaults());
        let mut spi = spi_mock(&expectations);
        // Hard reset: `set_low`, 2 µs, `set_high`, 50 ms (`NoopDelay`
        // swallows the timing; the pin order is what is asserted).
        let mut pin = PinMock::new(&[PinTx::set(PinState::Low), PinTx::set(PinState::High)]);
        let reader = Rc522::new(spi.clone(), Some(pin.clone()), NoopDelay::new());
        assert_eq!(reader.init(), Ok(()));
        spi.done();
        pin.done();
    }

    #[test]
    fn i3_init_aborts_when_soft_reset_stuck() {
        // `soft_reset` consumes write + 3 stuck polls and fails; no default
        // register write may follow (an extra call would panic on missing
        // expectation, an early stop would fail `done()`).
        let expectations = [
            soft_reset_write(),
            command_poll(0x10),
            command_poll(0x10),
            command_poll(0x10),
        ]
        .concat();
        let mut spi = spi_mock(&expectations);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.init(), Err(SpiError::Bus));
        spi.done();
    }

    /// Fails the Nth `Write` op — the spi `Mock` has no `with_error`, so a
    /// mid-`init` failure needs this fake. Shared counters prove the driver
    /// stops at the failure instead of continuing.
    struct FailOnNthWriteSpi {
        writes: std::rc::Rc<std::cell::Cell<usize>>,
        transactions: std::rc::Rc<std::cell::Cell<usize>>,
        fail_at: usize,
    }

    impl embedded_hal::spi::ErrorType for FailOnNthWriteSpi {
        type Error = embedded_hal::spi::ErrorKind;
    }

    impl SpiDeviceTrait for FailOnNthWriteSpi {
        fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
            self.transactions.set(self.transactions.get() + 1);
            for op in operations.iter() {
                if matches!(op, Operation::Write(_)) {
                    let w = self.writes.get() + 1;
                    self.writes.set(w);
                    if w >= self.fail_at {
                        return Err(embedded_hal::spi::ErrorKind::Other);
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn i4_init_aborts_on_mid_sequence_write_failure() {
        // 4th `Write` overall: `soft_reset` write, two defaults, then the
        // `MOD_WIDTH` write fails — 5 transactions attempted, never a 6th.
        let writes = std::rc::Rc::new(std::cell::Cell::new(0));
        let transactions = std::rc::Rc::new(std::cell::Cell::new(0));
        let reader = Rc522::new(
            FailOnNthWriteSpi {
                writes: writes.clone(),
                transactions: transactions.clone(),
                fail_at: 4,
            },
            None::<PinMock>,
            NoopDelay::new(),
        );
        assert_eq!(reader.init(), Err(SpiError::Bus));
        assert_eq!(transactions.get(), 5);
    }

    #[test]
    fn h1_pin_set_low_failure_aborts_init() {
        use embedded_hal_mock::eh1::MockError;
        // Hard reset runs before any SPI traffic, so SPI stays silent.
        let mut spi = spi_mock(&[]);
        let mut pin =
            PinMock::new(&[PinTx::set(PinState::Low)
                .with_error(MockError::Io(std::io::ErrorKind::NotConnected))]);
        let reader = Rc522::new(spi.clone(), Some(pin.clone()), NoopDelay::new());
        assert_eq!(reader.init(), Err(SpiError::Bus));
        spi.done();
        pin.done();
    }

    #[test]
    fn h2_read_regs_rx_align_1_boundary() {
        // `mask = 0xFE`; payload copy pinned at the low boundary.
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x84, 0x84, 0x00], vec![0x00, 0x55, 0xAA]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values = [0u8; 2];
        assert_eq!(reader.read_regs(0x04, &mut values, 1), Ok(()));
        assert_eq!(values, [0x55, 0xAA]);
        spi.done();
    }

    #[test]
    fn h3_read_regs_rx_align_7_boundary() {
        // `mask = 0x80`; payload copy pinned at the high boundary.
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x84, 0x84, 0x00], vec![0x00, 0xA5, 0x5A]),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut values = [0u8; 2];
        assert_eq!(reader.read_regs(0x04, &mut values, 7), Ok(()));
        assert_eq!(values, [0xA5, 0x5A]);
        spi.done();
    }

    #[test]
    fn h4_write_regs_64_byte_fifo_boundary() {
        let payload: Vec<u8> = (0..64u8).collect();
        let mut spi = spi_mock(&[
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![0x12]),
            SpiTx::write_vec(payload.clone()),
            SpiTx::transaction_end(),
        ]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.write_regs(0x09 << 1, &payload), Ok(()));
        spi.done();
    }

    // ---------------------------------------------------------------
    // Phase 2 — single-shot UID poll.
    // ---------------------------------------------------------------

    /// One `read_reg(reg)` returning `val`.
    fn p2_rd(reg: u8, val: u8) -> [SpiTx<u8>; 3] {
        [
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(vec![0x80 | reg, 0x00], vec![0x00, val]),
            SpiTx::transaction_end(),
        ]
    }

    /// One FIFO write (`write_regs(FIFO_DATA_REG, payload)`).
    fn p2_fifo_write(payload: &[u8]) -> Vec<SpiTx<u8>> {
        vec![
            SpiTx::transaction_start(),
            SpiTx::write_vec(vec![0x12]),
            SpiTx::write_vec(payload.to_vec()),
            SpiTx::transaction_end(),
        ]
    }

    /// One FIFO read of `rx_payload.len()` bytes.
    fn p2_fifo_read(rx_payload: &[u8]) -> [SpiTx<u8>; 3] {
        let mut tx = vec![0x92u8; rx_payload.len()];
        tx.push(0x00);
        let mut rx = vec![0x00u8];
        rx.extend_from_slice(rx_payload);
        [
            SpiTx::transaction_start(),
            SpiTx::transfer_in_place(tx, rx),
            SpiTx::transaction_end(),
        ]
    }

    /// Full successful `transceive` frame (IRQ ready, no error bits).
    fn p2_transceive(
        send: &[u8],
        tx_bits: u8,
        irq: u8,
        err: u8,
        rx_payload: &[u8],
        ctrl: u8,
    ) -> Vec<SpiTx<u8>> {
        let mut v = Vec::new();
        v.extend(reg_write(0x02, 0x00)); // PCD_IDLE
        v.extend(reg_write(0x08, 0x7F)); // clear interrupts
        v.extend(reg_write(0x14, 0x80)); // flush FIFO
        v.extend(p2_fifo_write(send));
        v.extend(reg_write(0x1A, tx_bits)); // BitFraming
        v.extend(reg_write(0x02, 0x0C)); // PCD_TRANSCEIVE
        v.extend(p2_rd(0x1A, tx_bits)); // StartSend: read
        v.extend(reg_write(0x1A, tx_bits | 0x80)); // StartSend: set
        v.extend(p2_rd(0x08, irq)); // COM_IRQ ready
        v.extend(p2_rd(0x0C, err)); // ERROR_REG clean
        v.extend(p2_rd(0x14, rx_payload.len() as u8)); // FIFO level
        v.extend(p2_fifo_read(rx_payload));
        v.extend(p2_rd(0x18, ctrl)); // CONTROL_REG: full last byte
        v
    }

    /// Successful `calculate_crc` frame returning `[lo, hi]`.
    fn p2_crc(data: &[u8], lo: u8, hi: u8) -> Vec<SpiTx<u8>> {
        let mut v = Vec::new();
        v.extend(reg_write(0x02, 0x00));
        v.extend(reg_write(0x0A, 0x04));
        v.extend(reg_write(0x14, 0x80));
        v.extend(p2_fifo_write(data));
        v.extend(reg_write(0x02, 0x03));
        v.extend(p2_rd(0x0A, 0x04)); // CRCIRq set
        v.extend(reg_write(0x02, 0x00));
        v.extend(p2_rd(0x44, lo)); // CRC_L
        v.extend(p2_rd(0x42, hi)); // CRC_H
        v
    }

    /// Antenna-on from powered-down (`0x00` -> `0x03`).
    fn p2_antenna_on() -> Vec<SpiTx<u8>> {
        let mut v = Vec::new();
        v.extend(p2_rd(0x28, 0x00));
        v.extend(reg_write(0x28, 0x03));
        v
    }

    /// Antenna-off from powered-up (`0x03` -> `0x00`).
    fn p2_antenna_off() -> Vec<SpiTx<u8>> {
        let mut v = Vec::new();
        v.extend(p2_rd(0x28, 0x03));
        v.extend(reg_write(0x28, 0x00));
        v
    }

    /// `ValuesAfterColl=1` clear (`COLL_REG & ~0x80`).
    fn p2_coll_clear() -> Vec<SpiTx<u8>> {
        let mut v = Vec::new();
        v.extend(p2_rd(0x1C, 0x00));
        v.extend(reg_write(0x1C, 0x00));
        v
    }

    const P2_CRC_LO: u8 = 0xAA;
    const P2_CRC_HI: u8 = 0xBB;

    /// Full 4-byte happy transcript for `uid4`: REQA -> anticollision ->
    /// CRC -> select (SAK `0x08`, no cascade) framed by antenna on/off.
    /// BCC is the xor of the UID bytes (ISO 14443-3 §6.5.3.1).
    fn p2_happy_uid(uid4: &[u8; 4]) -> Vec<SpiTx<u8>> {
        let bcc = uid4[0] ^ uid4[1] ^ uid4[2] ^ uid4[3];
        let mut v = Vec::new();
        v.extend(p2_antenna_on());
        v.extend(p2_coll_clear());
        v.extend(p2_transceive(&[0x26], 0x07, 0x30, 0x00, &[0x04, 0x00], 0x00));
        v.extend(p2_transceive(
            &[0x93, 0x20],
            0x00,
            0x30,
            0x00,
            &[uid4[0], uid4[1], uid4[2], uid4[3], bcc],
            0x00,
        ));
        v.extend(p2_crc(
            &[0x93, 0x70, uid4[0], uid4[1], uid4[2], uid4[3], bcc],
            P2_CRC_LO,
            P2_CRC_HI,
        ));
        v.extend(p2_transceive(
            &[
                0x93, 0x70, uid4[0], uid4[1], uid4[2], uid4[3], bcc, P2_CRC_LO, P2_CRC_HI,
            ],
            0x00,
            0x30,
            0x00,
            &[0x08, P2_CRC_LO, P2_CRC_HI],
            0x00,
        ));
        v.extend(p2_antenna_off());
        v
    }

    /// Full 4-byte happy transcript: REQA -> anticollision -> CRC ->
    /// select (SAK `0x08`, no cascade) framed by antenna on/off.
    /// UID `[0x11,0x22,0x33,0x44]`, BCC `0x44`.
    fn p2_happy_4byte() -> Vec<SpiTx<u8>> {
        p2_happy_uid(&[0x11, 0x22, 0x33, 0x44])
    }

    /// Empty scan transcript: antenna on, collision-bit clear, REQA ending
    /// at the timer IRQ bit, antenna off. `poll_uid` maps this to `Ok(None)`.
    fn p2_empty() -> Vec<SpiTx<u8>> {
        let mut exp = Vec::new();
        exp.extend(p2_antenna_on());
        exp.extend(p2_coll_clear());
        exp.extend(reg_write(0x02, 0x00));
        exp.extend(reg_write(0x08, 0x7F));
        exp.extend(reg_write(0x14, 0x80));
        exp.extend(p2_fifo_write(&[0x26]));
        exp.extend(reg_write(0x1A, 0x07));
        exp.extend(reg_write(0x02, 0x0C));
        exp.extend(p2_rd(0x1A, 0x07));
        exp.extend(reg_write(0x1A, 0x87));
        exp.extend(p2_rd(0x08, 0x01)); // TimerIRq: nothing in field
        exp.extend(p2_antenna_off());
        exp
    }

    #[test]
    fn p2_poll_uid_4byte_happy() {
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_uid(&mut uid), Ok(Some(4)));
        assert_eq!(&uid[..4], &[0x11, 0x22, 0x33, 0x44]);
        spi.done();
    }

    #[test]
    fn p2_poll_uid_str_returns_hyphen_hex() {
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut bin = [0u8; 10];
        let mut out = [0u8; 32];
        assert_eq!(reader.poll_uid_str(&mut bin, &mut out), Ok(Some(11)));
        assert_eq!(&out[..11], b"11-22-33-44");
        spi.done();
    }

    #[test]
    fn p2_poll_uid_str_small_buffer_is_protocol() {
        // SPI transcript is fully consumed (antenna off runs); only the
        // formatting step fails.
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut bin = [0u8; 10];
        let mut out = [0u8; 4];
        assert_eq!(
            reader.poll_uid_str(&mut bin, &mut out),
            Err(PollError::Protocol)
        );
        spi.done();
    }

    #[test]
    fn p2_poll_uid_no_tag_returns_none_and_powers_down() {
        // REQA transceive ends at the timer IRQ bit; `done()` proves the
        // trailing antenna-off still ran.
        let mut exp = Vec::new();
        exp.extend(p2_antenna_on());
        exp.extend(p2_coll_clear());
        exp.extend(reg_write(0x02, 0x00));
        exp.extend(reg_write(0x08, 0x7F));
        exp.extend(reg_write(0x14, 0x80));
        exp.extend(p2_fifo_write(&[0x26]));
        exp.extend(reg_write(0x1A, 0x07));
        exp.extend(reg_write(0x02, 0x0C));
        exp.extend(p2_rd(0x1A, 0x07));
        exp.extend(reg_write(0x1A, 0x87));
        exp.extend(p2_rd(0x08, 0x01)); // TimerIRq: nothing in field
        exp.extend(p2_antenna_off());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_uid(&mut uid), Ok(None));
        spi.done();
    }

    #[test]
    fn p2_poll_uid_short_atqa_is_protocol() {
        // REQA answers a single byte instead of the 2-byte ATQA.
        let mut exp = Vec::new();
        exp.extend(p2_antenna_on());
        exp.extend(p2_coll_clear());
        exp.extend(p2_transceive(&[0x26], 0x07, 0x30, 0x00, &[0x04], 0x00));
        exp.extend(p2_antenna_off());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_uid(&mut uid), Err(PollError::Protocol));
        spi.done();
    }

    #[test]
    fn p2_poll_uid_bcc_mismatch_is_protocol() {
        // Anticollision BCC byte corrupted: `0x44` -> `0x00`.
        let mut exp = Vec::new();
        exp.extend(p2_antenna_on());
        exp.extend(p2_coll_clear());
        exp.extend(p2_transceive(&[0x26], 0x07, 0x30, 0x00, &[0x04, 0x00], 0x00));
        exp.extend(p2_transceive(
            &[0x93, 0x20],
            0x00,
            0x30,
            0x00,
            &[0x11, 0x22, 0x33, 0x44, 0x00],
            0x00,
        ));
        exp.extend(p2_antenna_off());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_uid(&mut uid), Err(PollError::Protocol));
        spi.done();
    }

    #[test]
    fn p2_poll_uid_bus_error_maps() {
        let mut uid = [0u8; 10];
        assert_eq!(failing_reader().poll_uid(&mut uid), Err(PollError::Bus));
        assert_eq!(failing_reader().antenna_on(), Err(PollError::Bus));
        assert_eq!(failing_reader().antenna_off(), Err(PollError::Bus));
    }

    #[test]
    fn p2_calculate_crc_timeout() {
        // CRCIRq never sets across all 89 polls.
        let mut exp = Vec::new();
        exp.extend(reg_write(0x02, 0x00));
        exp.extend(reg_write(0x0A, 0x04));
        exp.extend(reg_write(0x14, 0x80));
        exp.extend(p2_fifo_write(&[0x01, 0x02]));
        exp.extend(reg_write(0x02, 0x03));
        for _ in 0..89 {
            exp.extend(p2_rd(0x0A, 0x00));
        }
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(
            reader.calculate_crc(&[0x01, 0x02]),
            Err(PollError::Timeout)
        );
        spi.done();
    }

    #[test]
    fn p2_calculate_crc_rejects_empty_and_oversize() {
        let mut spi = spi_mock(&[]);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.calculate_crc(&[]), Err(PollError::Protocol));
        let big = [0u8; 65];
        assert_eq!(reader.calculate_crc(&big), Err(PollError::Protocol));
        spi.done();
    }

    #[test]
    fn p2_antenna_idempotent_skips_redundant_write() {
        // Already on: single read, no write.
        let mut spi = spi_mock(&p2_rd(0x28, 0x03));
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.antenna_on(), Ok(()));
        spi.done();
        // Already off: single read, no write.
        let mut spi = spi_mock(&p2_rd(0x28, 0x00));
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        assert_eq!(reader.antenna_off(), Ok(()));
        spi.done();
    }

    #[test]
    fn p2_format_uid_hyphen_uppercase_hex() {
        let mut out = [0u8; 32];
        let n = Rc522::<SpiMock<u8>, PinMock, NoopDelay>::format_uid(
            &[0x74, 0x10, 0x37, 0x94],
            &mut out,
        );
        assert_eq!(n, 11);
        assert_eq!(&out[..n], b"74-10-37-94");
        // Single byte has no separator; empty UID writes nothing.
        let mut one = [0u8; 4];
        assert_eq!(
            Rc522::<SpiMock<u8>, PinMock, NoopDelay>::format_uid(&[0xAB], &mut one),
            2
        );
        assert_eq!(&one[..2], b"AB");
        let mut empty = [0u8; 4];
        assert_eq!(
            Rc522::<SpiMock<u8>, PinMock, NoopDelay>::format_uid(&[], &mut empty),
            0
        );
    }

    #[test]
    fn p2_format_uid_stops_at_last_whole_byte() {
        // 4-byte UID needs 11 chars; 4-char buffer holds only "11".
        let mut out = [0u8; 4];
        let n = Rc522::<SpiMock<u8>, PinMock, NoopDelay>::format_uid(
            &[0x11, 0x22, 0x33, 0x44],
            &mut out,
        );
        assert_eq!(n, 2);
        assert_eq!(&out[..n], b"11");
    }

    // ---------------------------------------------------------------
    // Phase 3 — presence match.
    // ---------------------------------------------------------------

    #[test]
    fn p3_uid_matches_exact_only() {
        assert!(TagPresence::uid_matches(&[0x11, 0x22], &[0x11, 0x22]));
        // Length mismatch never matches, even on a shared prefix.
        assert!(!TagPresence::uid_matches(&[0x11, 0x22], &[0x11, 0x22, 0x33]));
        assert!(!TagPresence::uid_matches(&[0x11, 0x22, 0x33], &[0x11, 0x22]));
        // Same length, one byte off.
        assert!(!TagPresence::uid_matches(
            &[0x11, 0x22, 0x33, 0x44],
            &[0x11, 0x22, 0x33, 0x45]
        ));
        assert!(!TagPresence::uid_matches(&[], &[0x11]));
    }

    #[test]
    fn p3_new_accepts_4_7_10_rejects_rest() {
        for len in [4usize, 7, 10] {
            let uid: Vec<u8> = (0..len as u8).collect();
            let sensor = TagPresence::new(&uid);
            assert!(sensor.is_some(), "len {len} must configure");
            assert_eq!(sensor.unwrap().uid(), &uid[..]);
        }
        for len in [0usize, 1, 3, 5, 6, 8, 9, 11] {
            let uid = [0x11u8; 11];
            assert!(
                TagPresence::new(&uid[..len]).is_none(),
                "len {len} must be rejected"
            );
        }
        // Fresh sensors start off.
        assert!(!TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap().is_present());
    }

    #[test]
    fn p3_process_match_turns_on_mismatch_turns_off() {
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert!(sensor.process(&[0x11, 0x22, 0x33, 0x44]));
        assert!(sensor.is_present());
        // A different tag in the field turns the sensor off immediately.
        assert!(!sensor.process(&[0xDE, 0xAD, 0xBE, 0xEF]));
        assert!(!sensor.is_present());
        // Back on when the configured tag returns.
        assert!(sensor.process(&[0x11, 0x22, 0x33, 0x44]));
        // Length mismatch also turns off (prefix is not a match).
        assert!(!sensor.process(&[0x11, 0x22, 0x33]));
        assert!(!sensor.is_present());
    }

    #[test]
    fn p3_scan_end_lingers_one_empty_scan() {
        // Ports `RC522BinarySensor::on_scan_end`: a sensor that matched
        // stays on through the first empty scan, then turns off.
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert!(sensor.process(&[0x11, 0x22, 0x33, 0x44]));
        assert!(sensor.on_scan_end());
        assert!(sensor.is_present());
        assert!(!sensor.on_scan_end());
        assert!(!sensor.is_present());
        // A sensor that never matched turns (stays) off at once.
        let mut fresh = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert!(!fresh.on_scan_end());
    }

    #[test]
    fn p3_update_folds_scan_option() {
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert!(sensor.update(Some(&[0x11, 0x22, 0x33, 0x44])));
        assert!(!sensor.update(Some(&[0x00, 0x00, 0x00, 0x00])));
        assert!(sensor.update(Some(&[0x11, 0x22, 0x33, 0x44])));
        assert!(sensor.update(None)); // first empty scan: still on
        assert!(!sensor.update(None)); // second empty scan: off
    }

    #[test]
    fn p3_parse_uid_happy() {
        let mut out = [0u8; 10];
        assert_eq!(TagPresence::parse_uid(b"74-10-37-94", &mut out), Some(4));
        assert_eq!(&out[..4], &[0x74, 0x10, 0x37, 0x94]);
        // Lowercase hex parses identically.
        let mut lower = [0u8; 10];
        assert_eq!(TagPresence::parse_uid(b"74-10-37-94".as_slice(), &mut lower), Some(4));
        let mut mixed = [0u8; 10];
        assert_eq!(TagPresence::parse_uid(b"ab-CD-ef-01", &mut mixed), Some(4));
        assert_eq!(&mixed[..4], &[0xAB, 0xCD, 0xEF, 0x01]);
        // 7- and 10-byte UIDs.
        let mut seven = [0u8; 10];
        assert_eq!(
            TagPresence::parse_uid(b"01-02-03-04-05-06-07", &mut seven),
            Some(7)
        );
        assert_eq!(&seven[..7], &[1, 2, 3, 4, 5, 6, 7]);
        let mut ten = [0u8; 10];
        assert_eq!(
            TagPresence::parse_uid(b"00-11-22-33-44-55-66-77-88-99", &mut ten),
            Some(10)
        );
    }

    #[test]
    fn p3_parse_uid_rejects_malformed() {
        let mut out = [0u8; 10];
        for bad in [
            b"".as_slice(),
            b"11-",           // trailing separator
            b"-11",           // leading separator
            b"1-22-33-44",    // single-digit group
            b"111-22-33-44",  // three-digit group
            b"GG-22-33-44",   // non-hex
            b"11 22 33 44",   // wrong separator
            b"11--22-33-44",  // doubled separator
            b"11223344",      // missing separators
            b"00-11-22-33-44-55-66-77-88-99-AA", // 11 groups
        ] {
            assert_eq!(TagPresence::parse_uid(bad, &mut out), None, "input: {bad:?}");
        }
    }

    #[test]
    fn p3_parse_round_trips_format_uid() {
        // `format_uid` output parses back to the same bytes.
        let uid = [0x74u8, 0x10, 0x37, 0x94];
        let mut text = [0u8; 32];
        let n = Rc522::<SpiMock<u8>, PinMock, NoopDelay>::format_uid(&uid, &mut text);
        let mut back = [0u8; 10];
        assert_eq!(TagPresence::parse_uid(&text[..n], &mut back), Some(4));
        assert_eq!(&back[..4], &uid);
    }

    #[test]
    fn p3_from_str_builds_configured_sensor() {
        let sensor = TagPresence::from_uid_str(b"11-22-33-44").unwrap();
        assert_eq!(sensor.uid(), &[0x11, 0x22, 0x33, 0x44]);
        assert!(!sensor.is_present());
        assert!(TagPresence::from_uid_str(b"11-22-33").is_none()); // 3 bytes
        assert!(TagPresence::from_uid_str(b"ZZ-22-33-44").is_none()); // malformed
    }

    #[test]
    fn p3_poll_presence_match_over_spi() {
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_presence(&mut sensor, &mut uid), Ok(true));
        assert!(sensor.is_present());
        assert_eq!(&uid[..4], &[0x11, 0x22, 0x33, 0x44]);
        spi.done();
    }

    #[test]
    fn p3_poll_presence_mismatch_over_spi() {
        // Field holds 11-22-33-44; sensor wants DE-AD-BE-EF.
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut sensor = TagPresence::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_presence(&mut sensor, &mut uid), Ok(false));
        assert!(!sensor.is_present());
        spi.done();
    }

    #[test]
    fn p3_poll_presence_empty_dwells_then_clears() {
        // Tag in, then two empty scans: on, lingering on, off.
        let mut exp = p2_happy_uid(&[0xAA, 0xBB, 0xCC, 0xDD]);
        exp.extend(p2_empty());
        exp.extend(p2_empty());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut sensor = TagPresence::new(&[0xAA, 0xBB, 0xCC, 0xDD]).unwrap();
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_presence(&mut sensor, &mut uid), Ok(true));
        assert_eq!(reader.poll_presence(&mut sensor, &mut uid), Ok(true));
        assert!(sensor.is_present());
        assert_eq!(reader.poll_presence(&mut sensor, &mut uid), Ok(false));
        assert!(!sensor.is_present());
        spi.done();
    }

    #[test]
    fn p3_poll_presence_bus_error_leaves_sensor_untouched() {
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert!(sensor.process(&[0x11, 0x22, 0x33, 0x44]));
        let mut uid = [0u8; 10];
        assert_eq!(
            failing_reader().poll_presence(&mut sensor, &mut uid),
            Err(PollError::Bus)
        );
        // Error scans never fold into the sensor.
        assert!(sensor.is_present());
    }

    // ---------------------------------------------------------------
    // Phase 4 — found/removed events.
    // ---------------------------------------------------------------

    #[test]
    fn p4_watcher_starts_empty() {
        let watcher = TagWatcher::new();
        assert!(!watcher.is_present());
        assert_eq!(watcher.current_uid(), None);
        assert_eq!(watcher.removed_uid(), &[][..]);
    }

    #[test]
    fn p4_process_new_uid_is_found_same_is_none() {
        let mut watcher = TagWatcher::new();
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::Found);
        assert!(watcher.is_present());
        assert_eq!(
            watcher.current_uid(),
            Some(&[0x11u8, 0x22, 0x33, 0x44][..])
        );
        // Same UID re-scanned: steady state, no retrigger.
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::None);
        assert!(watcher.is_present());
    }

    #[test]
    fn p4_process_swap_reports_found_only() {
        // Direct swap without an empty scan: Found for the new UID only
        // (ports `STATE_READ_SERIAL_DONE`: no `Removed` for the old UID).
        let mut watcher = TagWatcher::new();
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::Found);
        assert_eq!(watcher.process(&[0xDE, 0xAD, 0xBE, 0xEF]), TagEvent::Found);
        assert_eq!(
            watcher.current_uid(),
            Some(&[0xDEu8, 0xAD, 0xBE, 0xEF][..])
        );
        // Prefix with a different length is a different UID.
        assert_eq!(watcher.process(&[0xDE, 0xAD, 0xBE]), TagEvent::Found);
    }

    #[test]
    fn p4_scan_end_removes_once_then_quiet() {
        let mut watcher = TagWatcher::new();
        // Empty field with nothing tracked: quiet.
        assert_eq!(watcher.on_scan_end(), TagEvent::None);
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::Found);
        assert_eq!(watcher.on_scan_end(), TagEvent::Removed);
        assert!(!watcher.is_present());
        assert_eq!(watcher.current_uid(), None);
        assert_eq!(watcher.removed_uid(), &[0x11, 0x22, 0x33, 0x44]);
        // Second empty scan: quiet.
        assert_eq!(watcher.on_scan_end(), TagEvent::None);
    }

    #[test]
    fn p4_update_folds_scan_option() {
        let mut watcher = TagWatcher::new();
        assert_eq!(
            watcher.update(Some(&[0x11, 0x22, 0x33, 0x44])),
            TagEvent::Found
        );
        assert_eq!(
            watcher.update(Some(&[0x11, 0x22, 0x33, 0x44])),
            TagEvent::None
        );
        assert_eq!(watcher.update(None), TagEvent::Removed);
        assert_eq!(watcher.update(None), TagEvent::None);
    }

    #[test]
    fn p4_process_rejects_empty_and_oversize_without_state_change() {
        let mut watcher = TagWatcher::new();
        assert_eq!(watcher.process(&[]), TagEvent::None);
        assert!(!watcher.is_present());
        let big = [0xAAu8; 11];
        assert_eq!(watcher.process(&big), TagEvent::None);
        assert!(!watcher.is_present());
        // Tracked UID survives a junk scan.
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::Found);
        assert_eq!(watcher.process(&[]), TagEvent::None);
        assert_eq!(
            watcher.current_uid(),
            Some(&[0x11u8, 0x22, 0x33, 0x44][..])
        );
    }

    #[test]
    fn p4_callbacks_fire_once_per_tap_in_and_tap_out() {
        let mut watcher = TagWatcher::new();
        let mut found: Vec<[u8; 10]> = Vec::new();
        let mut found_len: Vec<usize> = Vec::new();
        let mut removed: Vec<[u8; 10]> = Vec::new();
        let mut removed_len: Vec<usize> = Vec::new();
        // Tap in: Found + callback.
        assert_eq!(watcher.update(Some(&[0x11, 0x22, 0x33, 0x44])), TagEvent::Found);
        if let Some(uid) = watcher.current_uid() {
            let mut slot = [0u8; 10];
            slot[..uid.len()].copy_from_slice(uid);
            found.push(slot);
            found_len.push(uid.len());
        }
        // Same tag still present: no retrigger, no callback.
        assert_eq!(watcher.update(Some(&[0x11, 0x22, 0x33, 0x44])), TagEvent::None);
        // Tap out: Removed + callback.
        assert_eq!(watcher.update(None), TagEvent::Removed);
        {
            let uid = watcher.removed_uid();
            let mut slot = [0u8; 10];
            slot[..uid.len()].copy_from_slice(uid);
            removed.push(slot);
            removed_len.push(uid.len());
        }
        // Field still empty: no retrigger, no callback.
        assert_eq!(watcher.update(None), TagEvent::None);
        assert_eq!(found.len(), 1);
        assert_eq!(&found[0][..found_len[0]], &[0x11, 0x22, 0x33, 0x44]);
        assert_eq!(removed.len(), 1);
        assert_eq!(&removed[0][..removed_len[0]], &[0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn p4_poll_watcher_tap_in_once_over_spi() {
        // Same UID polled twice: Found once, then steady None.
        let mut exp = p2_happy_4byte();
        exp.extend(p2_happy_4byte());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_watcher(&mut watcher, &mut uid), Ok(TagEvent::Found));
        assert_eq!(watcher.current_uid(), Some(&[0x11u8, 0x22, 0x33, 0x44][..]));
        assert_eq!(reader.poll_watcher(&mut watcher, &mut uid), Ok(TagEvent::None));
        spi.done();
    }

    #[test]
    fn p4_poll_watcher_tap_out_once_over_spi() {
        // Tag in, then two empty scans: Found, Removed, None.
        let mut exp = p2_happy_4byte();
        exp.extend(p2_empty());
        exp.extend(p2_empty());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut uid = [0u8; 10];
        assert_eq!(reader.poll_watcher(&mut watcher, &mut uid), Ok(TagEvent::Found));
        assert_eq!(reader.poll_watcher(&mut watcher, &mut uid), Ok(TagEvent::Removed));
        assert_eq!(watcher.removed_uid(), &[0x11, 0x22, 0x33, 0x44]);
        assert_eq!(reader.poll_watcher(&mut watcher, &mut uid), Ok(TagEvent::None));
        spi.done();
    }

    #[test]
    fn p4_poll_tag_events_fires_callbacks_once_over_spi() {
        use std::cell::Cell;
        use std::rc::Rc;
        let mut exp = p2_happy_4byte();
        exp.extend(p2_happy_4byte());
        exp.extend(p2_empty());
        exp.extend(p2_empty());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut uid = [0u8; 10];
        let found_count = Rc::new(Cell::new(0u32));
        let removed_count = Rc::new(Cell::new(0u32));
        let found_uid = Rc::new(Cell::new([0u8; 10]));
        let removed_uid = Rc::new(Cell::new([0u8; 10]));
        let mut on_found = {
            let found_count = found_count.clone();
            let found_uid = found_uid.clone();
            move |got: &[u8]| {
                found_count.set(found_count.get() + 1);
                let mut slot = [0u8; 10];
                slot[..got.len()].copy_from_slice(got);
                found_uid.set(slot);
            }
        };
        let mut on_removed = {
            let removed_count = removed_count.clone();
            let removed_uid = removed_uid.clone();
            move |got: &[u8]| {
                removed_count.set(removed_count.get() + 1);
                let mut slot = [0u8; 10];
                slot[..got.len()].copy_from_slice(got);
                removed_uid.set(slot);
            }
        };
        assert_eq!(
            reader.poll_tag_events(&mut watcher, &mut uid, &mut on_found, &mut on_removed),
            Ok(TagEvent::Found)
        );
        assert_eq!(
            reader.poll_tag_events(&mut watcher, &mut uid, &mut on_found, &mut on_removed),
            Ok(TagEvent::None)
        );
        assert_eq!(
            reader.poll_tag_events(&mut watcher, &mut uid, &mut on_found, &mut on_removed),
            Ok(TagEvent::Removed)
        );
        assert_eq!(
            reader.poll_tag_events(&mut watcher, &mut uid, &mut on_found, &mut on_removed),
            Ok(TagEvent::None)
        );
        assert_eq!(found_count.get(), 1);
        assert_eq!(removed_count.get(), 1);
        assert_eq!(&found_uid.get()[..4], &[0x11, 0x22, 0x33, 0x44]);
        assert_eq!(&removed_uid.get()[..4], &[0x11, 0x22, 0x33, 0x44]);
        spi.done();
    }

    #[test]
    fn p4_poll_watcher_bus_error_leaves_watcher_untouched() {
        let mut watcher = TagWatcher::new();
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::Found);
        let mut uid = [0u8; 10];
        assert_eq!(
            failing_reader().poll_watcher(&mut watcher, &mut uid),
            Err(PollError::Bus)
        );
        // Error scans never fold into the watcher and fire no callback.
        assert!(watcher.is_present());
        assert_eq!(
            watcher.current_uid(),
            Some(&[0x11u8, 0x22, 0x33, 0x44][..])
        );
        assert_eq!(
            failing_reader().poll_tag_events(
                &mut watcher,
                &mut uid,
                |_| panic!("found must not fire on error"),
                |_| panic!("removed must not fire on error"),
            ),
            Err(PollError::Bus)
        );
        assert!(watcher.is_present());
    }

    // ---------------------------------------------------------------
    // Phase 5 — timed re-scan loop.
    // ---------------------------------------------------------------

    #[test]
    fn p5_scan_interval_is_1s_and_text_fits_any_uid() {
        // Fixed 1 s re-scan interval (ESPHome `polling_component_schema("1s")`).
        assert_eq!(SCAN_INTERVAL_MS, 1000);
        // 10-byte UID renders as 29 chars; every pollable UID fits.
        assert_eq!(UID_TEXT_MAX, 29);
        let mut text = [0u8; UID_TEXT_MAX];
        let uid10 = [0xFFu8; 10];
        let n = Rc522::<SpiMock<u8>, PinMock, NoopDelay>::format_uid(&uid10, &mut text);
        assert_eq!(n, 29);
    }

    #[test]
    fn p5_poll_cycle_unknown_tag_logs_uid_text_and_fires_found() {
        use std::cell::Cell;
        use std::rc::Rc;
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        // Sensor wants a different tag, so 11-22-33-44 is unknown.
        let mut other = TagPresence::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        let mut sensors: [&mut TagPresence; 1] = [&mut other];
        let mut uid = [0u8; 10];
        let mut text = [0u8; UID_TEXT_MAX];
        let found_count = Rc::new(Cell::new(0u32));
        let removed_count = Rc::new(Cell::new(0u32));
        let found_count_ = found_count.clone();
        let removed_count_ = removed_count.clone();
        let event = reader
            .poll_cycle(
                &mut watcher,
                &mut sensors,
                &mut uid,
                &mut text,
                |_| found_count_.set(found_count_.get() + 1),
                |_| removed_count_.set(removed_count_.get() + 1),
            )
            .unwrap();
        assert_eq!(event, TagEvent::Found);
        // Unknown UID is formatted into the log buffer for enrollment copy-paste.
        assert_eq!(&text[..11], b"11-22-33-44");
        assert_eq!(found_count.get(), 1);
        assert_eq!(removed_count.get(), 0);
        // The non-matching sensor stays off.
        assert!(!sensors[0].is_present());
        spi.done();
    }

    #[test]
    fn p5_poll_cycle_known_tag_leaves_log_untouched() {
        use std::cell::Cell;
        use std::rc::Rc;
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut known = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        let mut sensors: [&mut TagPresence; 1] = [&mut known];
        let mut uid = [0u8; 10];
        let mut text = [0xAAu8; UID_TEXT_MAX];
        let found_count = Rc::new(Cell::new(0u32));
        let found_count_ = found_count.clone();
        let event = reader
            .poll_cycle(
                &mut watcher,
                &mut sensors,
                &mut uid,
                &mut text,
                |_| found_count_.set(found_count_.get() + 1),
                |_| panic!("removed must not fire on tap-in"),
            )
            .unwrap();
        assert_eq!(event, TagEvent::Found);
        assert_eq!(found_count.get(), 1);
        assert!(sensors[0].is_present());
        // Known tags are not logged: the buffer is byte-identical.
        assert_eq!(text, [0xAAu8; UID_TEXT_MAX]);
        spi.done();
    }

    #[test]
    fn p5_poll_cycle_repeat_scan_no_relog() {
        use std::cell::Cell;
        use std::rc::Rc;
        let mut exp = p2_happy_4byte();
        exp.extend(p2_happy_4byte());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut other = TagPresence::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        let mut sensors: [&mut TagPresence; 1] = [&mut other];
        let mut uid = [0u8; 10];
        let mut text = [0u8; UID_TEXT_MAX];
        let found_count = Rc::new(Cell::new(0u32));
        let found_count_ = found_count.clone();
        let mut on_found = |_: &[u8]| found_count_.set(found_count_.get() + 1);
        let mut on_removed = |_: &[u8]| panic!("removed must not fire while tag present");
        assert_eq!(
            reader.poll_cycle(&mut watcher, &mut sensors, &mut uid, &mut text, &mut on_found, &mut on_removed),
            Ok(TagEvent::Found)
        );
        assert_eq!(&text[..11], b"11-22-33-44");
        // Same tag still present: steady None, no retrigger, log buffer kept.
        assert_eq!(
            reader.poll_cycle(&mut watcher, &mut sensors, &mut uid, &mut text, &mut on_found, &mut on_removed),
            Ok(TagEvent::None)
        );
        assert_eq!(found_count.get(), 1);
        assert_eq!(&text[..11], b"11-22-33-44");
        spi.done();
    }

    #[test]
    fn p5_poll_cycle_empty_after_tap_fires_removed() {
        use std::cell::Cell;
        use std::rc::Rc;
        let mut exp = p2_happy_4byte();
        exp.extend(p2_empty());
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut other = TagPresence::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        let mut sensors: [&mut TagPresence; 1] = [&mut other];
        let mut uid = [0u8; 10];
        let mut text = [0u8; UID_TEXT_MAX];
        let removed_uid = Rc::new(Cell::new([0u8; 10]));
        let removed_uid_ = removed_uid.clone();
        let mut on_found = |_: &[u8]| {};
        assert_eq!(
            reader.poll_cycle(
                &mut watcher,
                &mut sensors,
                &mut uid,
                &mut text,
                &mut on_found,
                |_| panic!("removed must not fire on tap-in"),
            ),
            Ok(TagEvent::Found)
        );
        let mut on_found2 = |_: &[u8]| panic!("found must not fire on tap-out");
        let mut on_removed = |got: &[u8]| {
            let mut slot = [0u8; 10];
            slot[..got.len()].copy_from_slice(got);
            removed_uid_.set(slot);
        };
        assert_eq!(
            reader.poll_cycle(&mut watcher, &mut sensors, &mut uid, &mut text, &mut on_found2, &mut on_removed),
            Ok(TagEvent::Removed)
        );
        assert_eq!(&removed_uid.get()[..4], &[0x11, 0x22, 0x33, 0x44]);
        // The tap-in log buffer survives the tap-out cycle.
        assert_eq!(&text[..11], b"11-22-33-44");
        spi.done();
    }

    #[test]
    fn p5_poll_cycle_empty_field_stays_quiet() {
        let exp = p2_empty();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        let mut sensors: [&mut TagPresence; 1] = [&mut sensor];
        let mut uid = [0u8; 10];
        let mut text = [0xAAu8; UID_TEXT_MAX];
        assert_eq!(
            reader.poll_cycle(
                &mut watcher,
                &mut sensors,
                &mut uid,
                &mut text,
                |_| panic!("found must not fire on empty field"),
                |_| panic!("removed must not fire with nothing tracked"),
            ),
            Ok(TagEvent::None)
        );
        assert!(!sensors[0].is_present());
        assert_eq!(text, [0xAAu8; UID_TEXT_MAX]);
        spi.done();
    }

    #[test]
    fn p5_poll_cycle_one_of_many_sensors_marks_known() {
        let exp = p2_happy_4byte();
        let mut spi = spi_mock(&exp);
        let reader = Rc522::new(spi.clone(), None::<PinMock>, NoopDelay::new());
        let mut watcher = TagWatcher::new();
        let mut first = TagPresence::new(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        let mut second = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        let mut sensors: [&mut TagPresence; 2] = [&mut first, &mut second];
        let mut uid = [0u8; 10];
        let mut text = [0xAAu8; UID_TEXT_MAX];
        assert_eq!(
            reader.poll_cycle(
                &mut watcher,
                &mut sensors,
                &mut uid,
                &mut text,
                |_| {},
                |_| panic!("removed must not fire on tap-in"),
            ),
            Ok(TagEvent::Found)
        );
        // Exactly the matching sensor is on, and no unknown-tag log was written.
        assert!(!sensors[0].is_present());
        assert!(sensors[1].is_present());
        assert_eq!(text, [0xAAu8; UID_TEXT_MAX]);
        spi.done();
    }

    #[test]
    fn p5_poll_cycle_bus_error_leaves_state_untouched() {
        let mut watcher = TagWatcher::new();
        assert_eq!(watcher.process(&[0x11, 0x22, 0x33, 0x44]), TagEvent::Found);
        let mut sensor = TagPresence::new(&[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert!(sensor.process(&[0x11, 0x22, 0x33, 0x44]));
        let mut sensors: [&mut TagPresence; 1] = [&mut sensor];
        let mut uid = [0u8; 10];
        let mut text = [0xAAu8; UID_TEXT_MAX];
        assert_eq!(
            failing_reader().poll_cycle(
                &mut watcher,
                &mut sensors,
                &mut uid,
                &mut text,
                |_| panic!("found must not fire on error"),
                |_| panic!("removed must not fire on error"),
            ),
            Err(PollError::Bus)
        );
        // Error scans never fold into the watcher, sensors, or log buffer.
        assert!(watcher.is_present());
        assert_eq!(
            watcher.current_uid(),
            Some(&[0x11u8, 0x22, 0x33, 0x44][..])
        );
        assert!(sensors[0].is_present());
        assert_eq!(text, [0xAAu8; UID_TEXT_MAX]);
    }
}

//! Sensirion SCD40/SCD41 CO2, temperature and humidity sensor driver.
//!
//! The part answers at one fixed I2C address, which the schema declares
//! (`needs_address "0x62"`) so lowering can refuse a component wired to any
//! other one. The address is a constructor argument; the driver passes it to
//! every transaction rather than relying on an endpoint that rewrites what a
//! caller asks for.
//!
//! The part has a command interface: a command is a 16-bit word, and a reading
//! is a command followed by a read. Every data word carries a CRC-8 byte, so a
//! corrupt transfer is refused rather than reported as a measurement.

use esp_hal::Blocking;
use esp_hal::i2c::master::I2c;
use espforge_runtime::DeviceErrorKind;
use espforge_runtime::embedded_hal::i2c::I2c as _;
use espforge_runtime::embedded_hal_bus::i2c::RefCellDevice;

/// The I2C endpoint the generated wiring builds.
pub type Scd4xI2cEndpoint = RefCellDevice<'static, I2c<'static, Blocking>>;

/// `start_periodic_measurement`.
const START_PERIODIC_MEASUREMENT: u16 = 0x21b1;
/// `stop_periodic_measurement`.
const STOP_PERIODIC_MEASUREMENT: u16 = 0x3f86;
/// `read_measurement`.
const READ_MEASUREMENT: u16 = 0xec05;

/// Why a command or a reading failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scd4xError {
    /// The bus transfer failed.
    Bus,
    /// A data word failed its CRC check.
    Crc,
}

/// One measurement, in the driver's own fixed-point units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    /// CO2 concentration in parts per million.
    pub co2_ppm: u16,
    /// Temperature in hundredths of a degree Celsius.
    pub temperature_centi_c: i16,
    /// Relative humidity in hundredths of a percent.
    pub humidity_centi_percent: u16,
}

/// An SCD4x on an addressed I2C endpoint.
pub struct Scd4x {
    device: Scd4xI2cEndpoint,
    address: u8,
}

impl Scd4x {
    /// Take the shared endpoint and the address from the project file. The
    /// sensor needs [`start`](Self::start) before its first reading.
    ///
    /// Infallible: the part has no init sequence, so a dead sensor surfaces as
    /// a `Scd4xError` from `start()` rather than at construction.
    pub fn new(device: Scd4xI2cEndpoint, address: u8) -> Result<Self, DeviceErrorKind> {
        Ok(Self { device, address })
    }

    /// Begin periodic measurement (one sample every five seconds).
    pub fn start(&mut self) -> Result<(), Scd4xError> {
        self.command(START_PERIODIC_MEASUREMENT)
    }

    /// Stop periodic measurement, for example before a reconfiguration.
    pub fn stop(&mut self) -> Result<(), Scd4xError> {
        self.command(STOP_PERIODIC_MEASUREMENT)
    }

    /// Read the most recent measurement: three data words, each followed by its
    /// CRC byte.
    pub fn read(&mut self) -> Result<Measurement, Scd4xError> {
        let command = READ_MEASUREMENT.to_be_bytes();
        let mut raw = [0u8; 9];
        self.device
            .write_read(self.address, &command, &mut raw)
            .map_err(|_| Scd4xError::Bus)?;
        let co2 = word(&raw[0..3])?;
        let temperature = word(&raw[3..6])? as i16;
        let humidity = word(&raw[6..9])?;
        Ok(Measurement {
            co2_ppm: co2,
            // The datasheet's conversion: -45 + 175 * raw / 65536.
            temperature_centi_c: (temperature as i32 * 17_500 / 65_536 - 4_500) as i16,
            humidity_centi_percent: (humidity as u32 * 10_000 / 65_536) as u16,
        })
    }

    fn command(&mut self, command: u16) -> Result<(), Scd4xError> {
        self.device
            .write(self.address, &command.to_be_bytes())
            .map_err(|_| Scd4xError::Bus)
    }
}

/// One two-byte data word and its CRC byte.
fn word(raw: &[u8]) -> Result<u16, Scd4xError> {
    if crc8(&raw[0..2]) != raw[2] {
        return Err(Scd4xError::Crc);
    }
    Ok(u16::from_be_bytes([raw[0], raw[1]]))
}

/// The SCD4x CRC-8: polynomial 0x31, initial value 0xFF, no reflection.
fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0xffu8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x31
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The datasheet's worked example: `0xBEEF` has CRC `0x92`.
    #[test]
    fn the_crc_matches_the_datasheet_example() {
        assert_eq!(crc8(&[0xbe, 0xef]), 0x92);
    }

    #[test]
    fn a_word_with_a_bad_crc_is_refused() {
        assert_eq!(word(&[0xbe, 0xef, 0x92]), Ok(0xbeef));
        assert_eq!(word(&[0xbe, 0xef, 0x00]), Err(Scd4xError::Crc));
    }
}

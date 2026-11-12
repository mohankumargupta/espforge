//! Analog Devices ADXL345 three-axis accelerometer driver.
//!
//! A register-only part: `DEVID` (`0x00`) answers `0xE5`, and the three axes
//! are six bytes at `0x32`. The driver reads them over
//! `embedded_hal::spi::SpiDevice`, so it works with any endpoint the runtime
//! builds; the generated wiring gives it its own chip select and SPI mode.

use espforge_runtime::DeviceErrorKind;

/// The SPI endpoint the generated wiring builds.
///
/// Named apart from the display's `SpiEndpoint` so both drivers can be
/// re-exported from `espforge_drivers` without an ambiguous glob.
pub type Adxl345Endpoint = espforge_runtime::ConfiguredSpiDevice;

/// `DEVID`: the fixed identity register.
const DEVID: u8 = 0x00;
/// The value a healthy ADXL345 answers with.
const DEVID_VALUE: u8 = 0xE5;
/// `DATAX0`: X low byte, then Y and Z.
const DATAX0: u8 = 0x32;

/// Read bit: set for every read, cleared for a write.
const READ: u8 = 0x80;
/// Multi-byte bit: auto-increment the register address between bytes.
const MULTI: u8 = 0x40;

/// Why a transfer or identity check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adxl345Error {
    /// The bus rejected a transaction.
    Bus,
    /// The part answered, but not with the identity a ADXL345 has.
    WrongDevice,
    /// The part answered with fewer bytes than the register block needs.
    Short,
}

/// One sample, in the part's own counts (3.9 mg per count at the default
/// range). Kept as counts so the driver never needs floating point; use
/// [`Reading::milli_g`] for a scaled value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl Reading {
    /// The three axes in thousandths of a g, rounded to whole mg.
    pub fn milli_g(&self) -> (i32, i32, i32) {
        (milli_g(self.x), milli_g(self.y), milli_g(self.z))
    }
}

/// One axis in thousandths of a g. The datasheet's scale is 3.9 mg/LSB at the
/// default `±2 g` range; 3.9 is rounded to 4 here so the conversion is exact
/// integer arithmetic.
pub fn milli_g(counts: i16) -> i32 {
    counts as i32 * 4
}

/// The async SPI endpoint the generated wiring builds (`adxl345-async`).
#[cfg(feature = "adxl345-async")]
pub type AsyncAdxl345Endpoint = espforge_runtime::ConfiguredAsyncSpiDevice;

/// An ADXL345 on an SPI device endpoint.
pub struct Adxl345 {
    device: Adxl345Endpoint,
}

/// An ADXL345 whose transfers are async (`adxl345-async`).
///
/// Always built after boot: `Devices::new` is synchronous, so the generated
/// wiring stores a factory and the app awaits `init()`. The register protocol
/// is the same as [`Adxl345`]'s; only the endpoint's transfer is awaited.
#[cfg(feature = "adxl345-async")]
pub struct AsyncAdxl345 {
    device: AsyncAdxl345Endpoint,
}

#[cfg(feature = "adxl345-async")]
impl AsyncAdxl345 {
    /// Check the identity register and take the endpoint.
    pub async fn new(device: AsyncAdxl345Endpoint) -> Result<Self, DeviceErrorKind> {
        let mut this = Self { device };
        match this.device_id().await {
            Ok(DEVID_VALUE) => Ok(this),
            _ => Err(DeviceErrorKind::Init),
        }
    }

    /// The identity register: `0xE5` on an ADXL345.
    pub async fn device_id(&mut self) -> Result<u8, Adxl345Error> {
        let mut id = [0u8; 1];
        let command = [DEVID | READ];
        embedded_hal_async::spi::SpiDevice::transaction(
            &mut self.device,
            &mut [
                espforge_runtime::embedded_hal::spi::Operation::Write(&command),
                espforge_runtime::embedded_hal::spi::Operation::Read(&mut id),
            ],
        )
        .await
        .map_err(|_| Adxl345Error::Bus)?;
        Ok(id[0])
    }

    /// Read all three axes, awaiting the transfer.
    pub async fn read(&mut self) -> Result<Reading, Adxl345Error> {
        let mut raw = [0u8; 6];
        let command = [DATAX0 | READ | MULTI];
        embedded_hal_async::spi::SpiDevice::transaction(
            &mut self.device,
            &mut [
                espforge_runtime::embedded_hal::spi::Operation::Write(&command),
                espforge_runtime::embedded_hal::spi::Operation::Read(&mut raw),
            ],
        )
        .await
        .map_err(|_| Adxl345Error::Bus)?;
        decode(&raw)
    }
}

impl Adxl345 {
    /// Check the identity register and take the endpoint.
    ///
    /// A part that answers with another id is `DeviceErrorKind::Init`, so the
    /// boot log names the device instead of drawing bogus axes.
    pub fn new(device: Adxl345Endpoint) -> Result<Self, DeviceErrorKind> {
        let mut this = Self { device };
        match this.device_id() {
            Ok(DEVID_VALUE) => Ok(this),
            _ => Err(DeviceErrorKind::Init),
        }
    }

    /// The identity register: `0xE5` on an ADXL345.
    pub fn device_id(&mut self) -> Result<u8, Adxl345Error> {
        let mut id = [0u8; 1];
        self.device
            .write_read(&[DEVID | READ], &mut id)
            .map_err(|_| Adxl345Error::Bus)?;
        Ok(id[0])
    }

    /// Read all three axes.
    pub fn read(&mut self) -> Result<Reading, Adxl345Error> {
        let mut raw = [0u8; 6];
        self.device
            .write_read(&[DATAX0 | READ | MULTI], &mut raw)
            .map_err(|_| Adxl345Error::Bus)?;
        decode(&raw)
    }
}

/// Little-endian pairs, X then Y then Z.
fn decode(raw: &[u8]) -> Result<Reading, Adxl345Error> {
    if raw.len() < 6 {
        return Err(Adxl345Error::Short);
    }
    Ok(Reading {
        x: i16::from_le_bytes([raw[0], raw[1]]),
        y: i16::from_le_bytes([raw[2], raw[3]]),
        z: i16::from_le_bytes([raw[4], raw[5]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_are_little_endian_pairs() {
        // +1 g, -1 g and zero at the default range.
        let reading = decode(&[0x01, 0x01, 0xFF, 0xFE, 0x00, 0x00]).unwrap();
        assert_eq!(reading.x, 0x0101);
        assert_eq!(reading.y, -257);
        assert_eq!(reading.z, 0);
    }

    #[test]
    fn the_scale_is_four_milligrams_per_count() {
        assert_eq!(milli_g(0), 0);
        assert_eq!(milli_g(256), 1024);
        assert_eq!(milli_g(-256), -1024);
        let reading = Reading {
            x: 250,
            y: -250,
            z: 1,
        };
        assert_eq!(reading.milli_g(), (1000, -1000, 4));
    }

    #[test]
    fn a_short_read_is_refused() {
        assert_eq!(decode(&[0u8; 5]), Err(Adxl345Error::Short));
        assert!(decode(&[0u8; 6]).is_ok());
    }
}

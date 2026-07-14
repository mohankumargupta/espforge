//! `spi` component: an SPI master bus (ADR-003/008 bus-sharing).
//!
//! Wraps the esp-hal SPI master and attaches the chip-select pin so that every
//! transfer asserts/releases `cs` automatically (the wokwi chip expects active
//! low CS). Devices borrow `&SpiBus` (shared access) to talk on the same bus.

use esp_hal::spi::master::{Config, Spi};
use esp_hal::spi::Mode;
use esp_hal::gpio::{InputPin, OutputPin};

pub struct SpiBus {
    bus: Spi<'static, esp_hal::Blocking>,
}

impl SpiBus {
    /// `spi` is the SPI peripheral moved in by value; `mosi`/`miso`/`sclk`/`cs`
    /// are the bus pins moved in by value. `mode` is the SPI mode (0–3) and
    /// `frequency_khz` the bus clock. The CS pin is attached to the master so
    /// transfers manage it automatically.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        spi: esp_hal::peripherals::SPI2<'static>,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        sclk: impl OutputPin + 'static,
        cs: impl OutputPin + 'static,
        mode: u8,
        frequency_khz: u32,
    ) -> Self {
        let spi_mode = match mode {
            0 => Mode::_0,
            1 => Mode::_1,
            2 => Mode::_2,
            3 => Mode::_3,
            _ => Mode::_0,
        };
        let config = Config::default()
            .with_frequency(esp_hal::time::Rate::from_khz(frequency_khz))
            .with_mode(spi_mode);
        let bus = Spi::new(spi, config)
            .unwrap()
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso)
            .with_cs(cs);
        SpiBus { bus }
    }

    /// Shared access to the underlying bus (esp-hal `Spi`), which implements
    /// `embedded_hal::spi::SpiBus` so callers can use `transfer_in_place`, etc.
    pub fn bus(&self) -> &Spi<'static, esp_hal::Blocking> {
        &self.bus
    }

    /// Mutable access to the underlying bus.
    pub fn bus_mut(&mut self) -> &mut Spi<'static, esp_hal::Blocking> {
        &mut self.bus
    }

    /// Transfer `data` in place on the bus (full-duplex), managing CS. Mirrors
    /// `embedded_hal::spi::SpiBus::transfer_in_place` so example code reads
    /// naturally as `spi.transfer_in_place(&mut buf)`.
    pub fn transfer_in_place(&mut self, data: &mut [u8]) -> Result<(), esp_hal::spi::Error> {
        embedded_hal::spi::SpiBus::transfer_in_place(&mut self.bus, data)
    }
}

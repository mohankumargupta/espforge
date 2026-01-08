#![allow(unexpected_cfgs)]
use esp_hal::{
    gpio::AnyPin,
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
    Blocking,
};


pub struct SPIMaster {
    spi: Spi<'static, Blocking>,
}

impl SPIMaster {
    /// Creates a new SPI Master instance on SPI2.
    ///
    /// # Arguments
    /// * `sck` - The GPIO pin number for SCK
    /// * `mosi` - The GPIO pin number for MOSI
    /// * `miso` - The GPIO pin number for MISO
    /// * `frequency_khz` - The clock frequency in kHz
    ///
    /// # Panics
    /// Panics if the pins are invalid or if SPI2 is not available.
    pub fn new(spi_bus: u8, sck_pin: u8, mosi_pin: u8, miso_pin: u8, cs_pin: u8, frequency: u32, mode: u8) -> Self {
        let spi_mode = match mode {
            1 => Mode::_1,
            2 => Mode::_2,
            3 => Mode::_3,
            _ => Mode::_0,
        };

        let config = Config::default()
            .with_frequency(Rate::from_khz(frequency))
            .with_mode(spi_mode); 

        let driver = match spi_bus {
            2 => {
                let spi = unsafe { esp_hal::peripherals::SPI2::steal() };
                Spi::new(spi, config).unwrap()
            },
            #[cfg(any(feature = "esp32", feature = "esp32s2", feature = "esp32s3"))]
            3 => {
                let spi = unsafe { esp_hal::peripherals::SPI3::steal() };
                Spi::new(spi, config).unwrap()
            },
            _ => panic!("Invalid SPI bus number: {}. ESP32 typically uses 2 (FSPI) or 3 (HSPI).", spi_bus),
        };

        let sck = unsafe { AnyPin::steal(sck_pin) };
        //let miso = unsafe { AnyPin::steal(miso_pin) };
        let mosi = unsafe { AnyPin::steal(mosi_pin) };

        let mut driver = driver.with_sck(sck)
            //.with_miso(miso)
            .with_mosi(mosi);

        if miso_pin != u8::MAX {
            let miso = unsafe { AnyPin::steal(miso_pin) };
            driver = driver.with_miso(miso);
        }

        if cs_pin != u8::MAX {
             let cs = unsafe { AnyPin::steal(cs_pin) };
             driver = driver.with_cs(cs);
        }

        SPIMaster { spi: driver }
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.spi.write(bytes)
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<(), esp_hal::spi::Error> {
        self.spi.read(buffer)
    }

    pub fn transfer(&mut self, write: &mut[u8]) -> Result<(), esp_hal::spi::Error> {
        self.spi.transfer(write)
    }

    pub fn into_inner(self) -> Spi<'static, Blocking> {
        self.spi
    }
}
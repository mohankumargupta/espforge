use esp_hal::{
    gpio::AnyPin,
    peripherals::SPI2,
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
    Blocking,
};


/// User-friendly SPI Master wrapper
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
    pub fn new(sck: u8, mosi: u8, miso: u8, cs: u8) -> Self {
        // Safety: We ensure only one instance exists by consuming the AnyPin
        // In the context of espforge generated code, we assume this is called once.
        let spi = unsafe { SPI2::steal() };
        let sck_pin = unsafe { AnyPin::steal(sck) };
        let mosi_pin = unsafe { AnyPin::steal(mosi) };
        let miso_pin = unsafe { AnyPin::steal(miso) };
        //let cs_pin = unsafe { AnyPin::steal(cs) };
        let frequency_khz = 100;
        let config = Config::default()
            .with_frequency(Rate::from_khz(frequency_khz))
            .with_mode(Mode::_0); // Default to Mode 0 (CPOL=0, CPHA=0)

        let mut driver = Spi::new(spi, config)
            .unwrap()
            .with_sck(sck_pin)
            .with_miso(miso_pin)
            .with_mosi(mosi_pin);
            //.with_cs(cs_pin);

        if cs != u8::MAX {
             let cs_pin = unsafe { AnyPin::steal(cs) };
             driver = driver.with_cs(cs_pin);
        }

        SPIMaster { spi: driver }
    }

    /// Writes bytes to the bus. 
    /// Note: This does not assert a Chip Select (CS) pin.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.spi.write(bytes)
    }

    /// Reads bytes from the bus.
    /// Note: This does not assert a Chip Select (CS) pin.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<(), esp_hal::spi::Error> {
        self.spi.read(buffer)
    }

    // Performs a transfer (simultaneous write and read).
    // Note: This does not assert a Chip Select (CS) pin.
    pub fn transfer(&mut self, write: &mut[u8]) -> Result<(), esp_hal::spi::Error> {
        self.spi.transfer(write)
    }

    pub fn into_inner(self) -> Spi<'static, Blocking> {
        self.spi
    }

    // /// Helper to write to a specific device by handling Chip Select (CS) manually.
    // /// 
    // /// This creates a temporary Output for the CS pin, asserts it (Active Low), 
    // /// writes the data, and de-asserts it.
    // pub fn write_device(&mut self, cs_pin: u8, bytes: &[u8]) -> Result<(), esp_hal::spi::Error> {
    //     let cs_any = unsafe { AnyPin::steal(cs_pin) };
    //     let mut cs = Output::new(cs_any, Level::High, OutputConfig::default());
        
    //     cs.set_low();
    //     let result = self.spi.write(bytes);
    //     cs.set_high();
        
    //     result
    // }

    // /// Helper to read from a specific device by handling Chip Select (CS) manually.
    // pub fn read_device(&mut self, cs_pin: u8, buffer: &mut [u8]) -> Result<(), esp_hal::spi::Error> {
    //     let cs_any = unsafe { AnyPin::steal(cs_pin) };
    //     let mut cs = Output::new(cs_any, Level::High, OutputConfig::default());
        
    //     cs.set_low();
    //     let result = self.spi.read(buffer);
    //     cs.set_high();
        
    //     result
    // }

    // /// Helper to transfer to a specific device by handling Chip Select (CS) manually.
    // pub fn transfer_device(&mut self, cs_pin: u8, read: &mut [u8], write: &[u8]) -> Result<(), esp_hal::spi::Error> {
    //     let cs_any = unsafe { AnyPin::steal(cs_pin) };
    //     let mut cs = Output::new(cs_any, Level::High, OutputConfig::default());
        
    //     cs.set_low();
    //     let result = self.spi.transfer(read, write);
    //     cs.set_high();
        
    //     result
    // }
}


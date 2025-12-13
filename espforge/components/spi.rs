use crate::platform::spi::SPIMaster;

pub struct SPI {
    master: SPIMaster,
}

impl SPI {
    // We take cs as i64 because generic numbers in templates might be wider, but u8 is fine.
    // If cs is > 255 (e.g. -1 cast to unsigned), we assume no CS.
    pub fn new(sck: u8, mosi: u8, miso: u8, cs: u8) -> Self {
        SPI {
            master: SPIMaster::new(sck, mosi, miso, cs)
        }
    }


    pub fn write_read(&mut self, data: u8) -> u8 {
        let mut write_buffer = [data, 0x00];

        // Use the stored CS pin
        let _ = self.master.transfer(&mut write_buffer);

        // Return the byte received during the second cycle
        write_buffer[1]
    }

    pub fn into_inner(self) -> SPIMaster {
        self.master
    }
}


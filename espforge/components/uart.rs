use crate::platform::uart::UartDriver;
use core::str;

pub struct Uart {
    driver: UartDriver,
    rx_buffer: [u8; 128], 
    rx_len: usize,
}

impl Uart {
    pub fn new(uart: u8, tx: u8, rx: u8, baud: u32) -> Self {
        Self {
            driver: UartDriver::new(uart, tx, rx, baud),
            rx_buffer: [0u8; 128],
            rx_len: 0,
        }
    }

    pub fn write(&mut self, data: &str) {
        self.driver.write(data.as_bytes());
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        self.driver.write(data);
    }

    pub fn available(&mut self) -> bool {
        self.driver.read_ready()
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        if self.available() {
            let mut buf = [0u8; 1];
            if self.driver.read(&mut buf) > 0 {
                return Some(buf[0]);
            }
        }
        None
    }

    pub fn buffer_until_newline(&mut self) -> bool {
        if let Some(byte) = self.read_byte() {
            if byte == b'\n' {
                return true; 
            }
            
            if self.rx_len < self.rx_buffer.len() {
                self.rx_buffer[self.rx_len] = byte;
                self.rx_len += 1;
            }
        }
        false
    }

    pub fn get_buffered_string(&mut self) -> &str {
        match str::from_utf8(&self.rx_buffer[0..self.rx_len]) {
            Ok(s) => s,
            Err(_) => "Invalid UTF-8",
        }
    }

    pub fn clear_buffer(&mut self) {
        self.rx_len = 0;
    }
}


use esp_hal::{
    gpio::AnyPin,
    uart::{Config, Uart},
    peripherals::UART1,
    Blocking,
};

/// User-friendly UART Driver wrapper
pub struct UartDriver {
    uart: Uart<'static, Blocking>,
}

impl UartDriver {
    /// Creates a new UART driver using UART0
    ///
    /// # Arguments
    /// * `tx` - The GPIO pin number for TX
    /// * `rx` - The GPIO pin number for RX
    /// * `baud` - The baud rate
    pub fn new(tx: u8, rx: u8, baud: u32) -> Self {
        // Safety: We ensure only one instance exists by consuming the peripherals via steal
        let uart_peri = unsafe { UART1::steal() };
        let tx_pin = unsafe { AnyPin::steal(tx) };
        let rx_pin = unsafe { AnyPin::steal(rx) };

        let config = Config::default().with_baudrate(baud);

        let uart = Uart::new(uart_peri, config)
            .unwrap()
            .with_tx(tx_pin)
            .with_rx(rx_pin);

        Self { uart }
    }

    /// Writes bytes to the UART
    pub fn write(&mut self, data: &[u8]) {
        // We ignore errors for simplicity in this wrapper
        let _ = self.uart.write(data);
    }

    /// Reads data into the buffer. Returns the number of bytes read.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        match self.uart.read(buf) {
            Ok(len) => len,
            Err(_) => 0,
        }
    }

    /// Returns true if there is data available in the RX buffer
    pub fn read_ready(&mut self) -> bool {
        self.uart.read_ready()
    }
}


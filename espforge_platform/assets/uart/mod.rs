#![allow(unexpected_cfgs)]
use esp_hal::{
    gpio::AnyPin,
    uart::{Config, Uart},
    Blocking,
};
use embedded_io::{ErrorType, Read, Write, ReadReady, WriteReady};

pub struct UartDriver {
    uart: Uart<'static, Blocking>,
}

impl UartDriver {
    /// Creates a new UART driver using UART0
    ///
    /// # Arguments
    /// * `uart` - The UART bus number
    /// * `tx` - The GPIO pin number for TX
    /// * `rx` - The GPIO pin number for RX
    /// * `baud` - The baud rate
    pub fn new(uart_num: u8, tx: u8, rx: u8, baud: u32) -> Self {
        let tx_pin = unsafe { AnyPin::steal(tx) };
        let rx_pin = unsafe { AnyPin::steal(rx) };

        let config = Config::default().with_baudrate(baud);

        let uart_driver = match uart_num {
            0 => {
                let peri = unsafe { esp_hal::peripherals::UART0::steal() };
                Uart::new(peri, config).unwrap()
            },
            #[cfg(any(feature = "esp32", feature = "esp32s2", feature = "esp32s3", feature = "esp32c3", feature = "esp32c6", feature = "esp32h2"))]
            1 => {
                let peri = unsafe { esp_hal::peripherals::UART1::steal() };
                Uart::new(peri, config).unwrap()
            },
             #[cfg(any(feature = "esp32", feature = "esp32s3"))]
            2 => {
                 let peri = unsafe { esp_hal::peripherals::UART2::steal() };
                 Uart::new(peri, config).unwrap()
            },
            _ => panic!("Invalid UART bus number: {}", uart_num),
        };        

        let uart = uart_driver
            .with_tx(tx_pin)
            .with_rx(rx_pin);

        Self { uart }
    }
}

impl ErrorType for UartDriver {
    type Error = <Uart<'static, Blocking> as ErrorType>::Error;
}

impl Read for UartDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.uart.read(buf)
    }
}

impl ReadReady for UartDriver {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        self.uart.read_ready()
    }
}

impl Write for UartDriver {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.uart.write(buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.uart.flush()
    }
}

impl WriteReady for UartDriver {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        self.uart.write_ready()
    }
}
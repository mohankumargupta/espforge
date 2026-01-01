use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct Esp32Config {
    #[serde(default)]
    pub gpio: HashMap<String, GpioPinConfig>,
    #[serde(default)]
    pub spi: HashMap<String, SpiConfig>,
    #[serde(default)]
    pub i2c: HashMap<String, I2cConfig>,
    #[serde(default)]
    pub uart: HashMap<String, UartConfig>,
}

impl Esp32Config {
    /// Checks if a resource name exists in any of the configured peripheral maps
    pub fn contains_resource(&self, name: &str) -> bool {
        self.gpio.contains_key(name)
            || self.spi.contains_key(name)
            || self.i2c.contains_key(name)
            || self.uart.contains_key(name)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UartConfig {
    #[serde(default)]
    pub uart: u8,
    pub tx: u8,
    pub rx: u8,
    #[serde(default = "default_uart_baud")]
    pub baud: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SpiConfig {
    #[serde(default)]
    pub spi: u8,
    #[serde(default)]
    pub miso: Option<u8>,
    pub mosi: u8,
    pub sck: u8,
    #[serde(default)]
    pub cs: Option<u8>,
    #[serde(
        default = "default_spi_frequency",
        alias = "frequency_kHz",
        alias = "frequency_khz"
    )]
    pub frequency: u32,
    #[serde(default)]
    pub mode: u8,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct I2cConfig {
    #[serde(default)]
    pub i2c: u8,
    pub sda: u8,
    pub scl: u8,
    #[serde(
        default = "default_i2c_frequency",
        alias = "frequency_kHz",
        alias = "frequency_khz"
    )]
    pub frequency: u32,
}

fn default_i2c_frequency() -> u32 {
    100
}

fn default_spi_frequency() -> u32 {
    1000
}

fn default_uart_baud() -> u32 {
    9600
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct GpioPinConfig {
    pub pin: u8,
    pub direction: PinDirection,
    #[serde(default)]
    pub pullup: bool,
    #[serde(default)]
    pub pulldown: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum PinDirection {
    Input,
    Output,
}

use std::{collections::HashMap, fmt};

use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct EspforgeConfiguration {
    pub espforge: EspforgeConfig,
    #[serde(default)]
    pub example: Option<ExampleConfig>,
    pub esp32: Option<Esp32Config>,
    pub components: Option<HashMap<String, ComponentConfig>>,
    pub devices: Option<HashMap<String, DeviceConfig>>,
    pub app: Option<AppConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeviceConfig {
    pub using: String,
    #[serde(default)]
    pub with: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EspforgeConfig {
    pub name: String,
    pub platform: PlatformConfig,
    #[serde(default)]
    pub wokwi_board: Option<WokwiBoard>,
    #[serde(default)]
    pub wokwi: Option<WokwiConfig>,
    #[serde(default)]
    pub enable_async: bool,

}

#[derive(Debug, Deserialize, Serialize)]
pub struct WokwiConfig {
    pub diagram: Option<String>,
    pub config: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WokwiBoard {
    BoardEsp32DevkitCV4,
    BoardEsp32S2Devkitm1,
    BoardEsp32S3Devkitc1,
    BoardEsp32C3Devkitm1,
    BoardEsp32C6Devkitc1,
    BoardEsp32H2Devkitm1,
    BoardXiaoEsp32C3,
    BoardXiaoEsp32C6,
    BoardXiaoEsp32S3,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExampleConfig {
    pub name: String,
    #[serde(flatten)]
    pub example_properties: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformConfig {
    ESP32,
    ESP32C2,
    ESP32C3,
    ESP32C6,
    ESP32H2,
    ESP32S2,
    ESP32S3,
}

impl PlatformConfig {
    pub fn target(&self) -> &str {
        match self {
            PlatformConfig::ESP32 => "xtensa-esp32-none-elf",
            PlatformConfig::ESP32C2 => "riscv32imc-unknown-none-elf",
            PlatformConfig::ESP32C3 => "riscv32imc-unknown-none-elf",
            PlatformConfig::ESP32C6 => "riscv32imac-unknown-none-elf",
            PlatformConfig::ESP32H2 => "riscv32imac-unknown-none-elf",
            PlatformConfig::ESP32S2 => "xtensa-esp32s2-none-elf",
            PlatformConfig::ESP32S3 => "xtensa-esp32s3-none-elf",
        }
    }
}

impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        &self.espforge.name
    }

    pub fn get_platform(&self) -> String {
        self.espforge.platform.to_string()
    }

    pub fn get_template(&self) -> Option<String> {
        self.example.as_ref().map(|e| e.name.clone())
    }
}

impl fmt::Display for PlatformConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PlatformConfig::ESP32 => write!(f, "esp32"),
            PlatformConfig::ESP32C2 => write!(f, "esp32c2"),
            PlatformConfig::ESP32C3 => write!(f, "esp32c3"),
            PlatformConfig::ESP32C6 => write!(f, "esp32c6"),
            PlatformConfig::ESP32H2 => write!(f, "esp32h2"),
            PlatformConfig::ESP32S2 => write!(f, "esp32s2"),
            PlatformConfig::ESP32S3 => write!(f, "esp32s3"),
        }
    }
}

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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UartConfig {
    #[serde(default)]
    pub uart: u8,
    pub tx: u8,
    pub rx: u8,
    #[serde(default = "default_uart_baud")]
    pub baud: u32,    
}

impl Esp32Config {
    /// Checks if a resource name exists in any of the configured peripheral maps
    pub fn contains_resource(&self, name: &str) -> bool {
        self.gpio.contains_key(name) ||
        self.spi.contains_key(name) ||
        self.i2c.contains_key(name) ||
        self.uart.contains_key(name)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SpiConfig {
    #[serde(default)]
    pub spi: u8,    
    pub miso: u8,
    pub mosi: u8,
    pub sck: u8,
    #[serde(default)]
    pub cs: Option<u8>,
    #[serde(default = "default_i2c_frequency", alias = "frequency_kHz", alias = "frequency_khz")]
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
    #[serde(default = "default_i2c_frequency", alias = "frequency_kHz", alias = "frequency_khz")]
    pub frequency: u32,
}

fn default_i2c_frequency() -> u32 {
    100
}

fn default_uart_baud() -> u32 {
    9600
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GpioPinConfig {
    pub pin: u8,
    pub direction: PinDirection,
    #[serde(default)]
    pub pullup: bool,
    #[serde(default)]
    pub pulldown: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PinDirection {
    Input,
    Output,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ComponentConfig {
    pub using: String,
    #[serde(default)]
    pub with: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub variables: HashMap<String, VariableConfig>,

    #[serde(default)]
    pub setup: Vec<HashMap<String, Value>>,

    #[serde(default, rename = "loop")]
    pub loop_fn: Vec<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VariableConfig {
    #[serde(rename = "type")]
    pub type_name: String,
    pub initial: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml_ng;

    #[test]
    fn parse_minimal() {
        let yaml = r#"
            espforge:
              name: minimum
              platform: esp32c3
        "#;

        let config: EspforgeConfiguration =
            serde_yaml_ng::from_str(yaml).expect("YAML parse failed");

        assert_eq!(config.espforge.name, "minimum");
        assert_eq!(config.espforge.platform, PlatformConfig::ESP32C3);
        assert!(config.example.is_none());
    }

    #[test]
    fn parse_wokwi_config() {
        let yaml = r#"
            espforge:
              name: wokwi_test
              platform: esp32c3
              wokwi:
                diagram: diagram.json
                config: wokwi.toml
        "#;

        let config: EspforgeConfiguration =
            serde_yaml_ng::from_str(yaml).expect("YAML parse failed");

        let wokwi = config.espforge.wokwi.expect("Wokwi config should exist");
        assert_eq!(wokwi.diagram, Some("diagram.json".to_string()));
        assert_eq!(wokwi.config, Some("wokwi.toml".to_string()));
    }

    #[test]
    fn invalid_platform() {
        let yaml = r#"
            espforge:
              name: minimum
              platform: invalid
        "#;

        let result: Result<EspforgeConfiguration, _> = serde_yaml_ng::from_str(yaml);
        assert!(
            result.is_err(),
            "expected invalid_platform to fail deserialization"
        );
    }

    #[test]
    fn parse_blink_example() {
        let yaml = r#"
            espforge:
              name: blink
              platform: esp32c3
            example:
              name: blink            
        "#;

        let config: EspforgeConfiguration =
            serde_yaml_ng::from_str(yaml).expect("YAML parse failed");

        assert_eq!(config.espforge.name, "blink");
        if let Some(example) = config.example {
            assert_eq!(example.name, "blink");
        } else {
            panic!("Example was None.");
        }
    }
}
use serde::{Deserialize, Serialize};
use std::fmt;

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

use crate::config::SpiConfig;
use serde::Serialize;

#[derive(Serialize)]
pub struct SpiTemplateView {
    pub spi: u8,
    pub miso: u8,
    pub mosi: u8,
    pub sck: u8,
    pub cs: u8,
    pub frequency: u32,
    pub mode: u8,
}

impl From<&SpiConfig> for SpiTemplateView {
    fn from(config: &SpiConfig) -> Self {
        Self {
            spi: config.spi,
            miso: config.miso.unwrap_or(u8::MAX),
            mosi: config.mosi,
            sck: config.sck,
            cs: config.cs.unwrap_or(u8::MAX),
            frequency: config.frequency,
            mode: config.mode,
        }
    }
}


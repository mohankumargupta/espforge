#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SpiDeviceConfig {
    #[cfg(feature = "std")]
    pub spi: String,
}

impl Default for SpiDeviceConfig {
    fn default() -> Self {
        Self {
            #[cfg(feature = "std")]
            spi: String::new(),
        }
    }
}

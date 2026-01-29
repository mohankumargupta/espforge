#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UartDeviceConfig {
    #[cfg(feature = "std")]
    pub uart: String,
    pub baud: Option<u32>,
}

impl Default for UartDeviceConfig {
    fn default() -> Self {
        Self {
            #[cfg(feature = "std")]
            uart: String::new(),
            baud: None,
        }
    }
}

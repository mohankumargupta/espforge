#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct I2cDeviceConfig {
    #[cfg(feature = "std")]
    pub i2c: String,
}

impl Default for I2cDeviceConfig {
    fn default() -> Self {
        Self {
            #[cfg(feature = "std")]
            i2c: String::new(),
        }
    }
}

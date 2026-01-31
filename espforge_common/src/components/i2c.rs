#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
use crate::ConfigString;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct I2cDeviceConfig {
    #[cfg(feature = "std")]
    pub i2c: ConfigString,
}

#[cfg(not(feature = "std"))]
impl Default for I2cDeviceConfig {
    fn default() -> Self {
        Self {}
    }
}

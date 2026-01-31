#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
use crate::ConfigString;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct LedConfig {
    #[cfg(feature = "std")]
    pub gpio: ConfigString,
    pub active_low: bool,
}

#[cfg(not(feature = "std"))]
impl Default for LedConfig {
    fn default() -> Self {
        Self {
            active_low: false,
        }
    }
}
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct LedConfig {
    #[cfg(feature = "std")]
    pub gpio: String,
    pub active_low: bool,
}

#[cfg(not(feature = "std"))]
impl Default for LedConfig {
    fn default() -> Self {
        Self {
            // gpio field does not exist when std is disabled
            active_low: false,
        }
    }
}
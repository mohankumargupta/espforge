#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
use crate::ConfigString;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct ButtonConfig {
    #[cfg(feature = "std")]
    pub gpio: ConfigString,
    #[cfg_attr(feature = "serde", serde(default))]
    pub pull_up: bool,

    #[cfg_attr(feature = "serde", serde(default))]
    pub pull_down: bool,
}

#[cfg(not(feature = "std"))]
impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            pull_up: false,
            pull_down: false,
        }
    }
}

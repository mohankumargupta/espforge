use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsramConfig {
    #[serde(default = "default_psram_mode")]
    pub mode: PsramMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PsramMode {
    Quad,
    Octal,
    Hex,
}

fn default_psram_mode() -> PsramMode {
    PsramMode::Quad
}

impl Default for PsramConfig {
    fn default() -> Self {
        Self {
            mode: default_psram_mode(),
        }
    }
}

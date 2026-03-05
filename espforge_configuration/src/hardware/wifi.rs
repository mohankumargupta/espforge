use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    Wpa2,
    Open,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WifiConfig {
    pub ssid: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub auth: AuthMode,
}

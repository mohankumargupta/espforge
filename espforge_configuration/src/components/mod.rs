use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Component {
    #[serde(alias = "using")]
    pub driver: String,

    #[serde(alias = "with", default)]
    pub properties: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Device {
    #[serde(alias = "using")]
    pub driver: String,

    #[serde(alias = "with", default)]
    pub properties: Value,
}

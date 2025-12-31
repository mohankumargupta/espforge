use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct DeviceConfig {
    pub using: String,
    #[serde(default)]
    pub with: HashMap<String, Value>,
}


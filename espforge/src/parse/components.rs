use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct ComponentConfig {
    pub using: String,
    #[serde(default)]
    pub with: HashMap<String, Value>,
}

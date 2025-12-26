use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub variables: HashMap<String, VariableConfig>,

    #[serde(default)]
    pub setup: Vec<HashMap<String, Value>>,

    #[serde(default, rename = "loop")]
    pub loop_fn: Vec<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VariableConfig {
    #[serde(rename = "type")]
    pub type_name: String,
    pub initial: Value,
}


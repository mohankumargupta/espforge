use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct ExampleConfig {
    pub name: String,
    #[serde(flatten)]
    pub example_properties: HashMap<String, Value>,
}
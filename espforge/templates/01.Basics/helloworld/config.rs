use serde::{Deserialize, Serialize};

pub fn default_message() -> String {
    String::from("Hello World")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
}

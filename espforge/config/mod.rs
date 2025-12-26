use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub mod app;
pub mod components;
pub mod devices;
pub mod esp32;
pub mod example;
pub mod project;

pub use app::*;
pub use components::*;
pub use devices::*;
pub use esp32::*;
pub use example::*;
pub use project::*;

#[derive(Debug, Deserialize, Serialize)]
pub struct EspforgeConfiguration {
    pub espforge: EspforgeConfig,
    #[serde(default)]
    pub example: Option<ExampleConfig>,
    pub esp32: Option<Esp32Config>,
    pub components: Option<HashMap<String, ComponentConfig>>,
    pub devices: Option<HashMap<String, DeviceConfig>>,
    pub app: Option<AppConfig>,
}

impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        &self.espforge.name
    }

    pub fn get_platform(&self) -> String {
        self.espforge.platform.to_string()
    }

    pub fn get_template(&self) -> Option<String> {
        self.example.as_ref().map(|e| e.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml_ng;

    #[test]
    fn parse_minimal() {
        let yaml = r#"
            espforge:
              name: minimum
              platform: esp32c3
        "#;

        let config: EspforgeConfiguration =
            serde_yaml_ng::from_str(yaml).expect("YAML parse failed");

        assert_eq!(config.espforge.name, "minimum");
        assert_eq!(config.espforge.platform, PlatformConfig::ESP32C3);
        assert!(config.example.is_none());
    }

    #[test]
    fn parse_wokwi_config() {
        let yaml = r#"
            espforge:
              name: wokwi_test
              platform: esp32c3
              wokwi:
                diagram: diagram.json
                config: wokwi.toml
        "#;

        let config: EspforgeConfiguration =
            serde_yaml_ng::from_str(yaml).expect("YAML parse failed");

        let wokwi = config.espforge.wokwi.expect("Wokwi config should exist");
        assert_eq!(wokwi.diagram, Some("diagram.json".to_string()));
        assert_eq!(wokwi.config, Some("wokwi.toml".to_string()));
    }

    #[test]
    fn invalid_platform() {
        let yaml = r#"
            espforge:
              name: minimum
              platform: invalid
        "#;

        let result: Result<EspforgeConfiguration, _> = serde_yaml_ng::from_str(yaml);
        assert!(
            result.is_err(),
            "expected invalid_platform to fail deserialization"
        );
    }

    #[test]
    fn parse_blink_example() {
        let yaml = r#"
            espforge:
              name: blink
              platform: esp32c3
            example:
              name: blink            
        "#;

        let config: EspforgeConfiguration =
            serde_yaml_ng::from_str(yaml).expect("YAML parse failed");

        assert_eq!(config.espforge.name, "blink");
        if let Some(example) = config.example {
            assert_eq!(example.name, "blink");
        } else {
            panic!("Example was None.");
        }
    }
}

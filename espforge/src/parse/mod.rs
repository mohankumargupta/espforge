use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::parse::{
    app::AppConfig, components::ComponentConfig, devices::DeviceConfig, esp32::Esp32Config,
    project::EspforgeConfig,
};

pub mod app;
pub mod components;
pub mod devices;
pub mod esp32;
pub mod project;

#[derive(Debug, Deserialize, Serialize)]
pub struct EspforgeConfiguration {
    pub espforge: EspforgeConfig,
    #[serde(default)]
    pub esp32: Option<Esp32Config>,
    pub components: Option<HashMap<String, ComponentConfig>>,
    pub devices: Option<HashMap<String, DeviceConfig>>,
    pub app: Option<AppConfig>,
}

impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        &self.espforge.name
    }

    pub fn get_chip(&self) -> String {
        self.espforge.chip.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::parse::project::ChipConfig;

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
        assert_eq!(config.espforge.chip, ChipConfig::ESP32C3);
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
    }
}

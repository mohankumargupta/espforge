use crate::{
    config::EspforgeConfiguration,
    nibblers::{ConfigNibbler, NibblerResult, NibblerStatus},
    register_nibbler,
};
use espforge_macros::auto_register_nibbler;
use serde_yaml_ng::Value;

#[derive(Default)]
#[auto_register_nibbler]
pub struct ComponentNibbler;

impl ConfigNibbler for ComponentNibbler {
    fn name(&self) -> &str {
        "ComponentNibbler"
    }

    fn priority(&self) -> u8 {
        20
    }

    fn process(&self, config: &EspforgeConfiguration) -> Result<NibblerResult, String> {
        let components = match &config.components {
            Some(comps) => comps,
            None => return Ok(self.empty_result()),
        };

        let (findings, status) = self.validate_components(components, config);

        Ok(NibblerResult {
            nibbler_name: self.name().to_string(),
            findings,
            status,
        })
    }
}

impl ComponentNibbler {
    fn empty_result(&self) -> NibblerResult {
        NibblerResult {
            nibbler_name: self.name().to_string(),
            findings: Vec::new(),
            status: NibblerStatus::Ok,
        }
    }

    fn validate_components(
        &self,
        components: &std::collections::HashMap<String, crate::config::ComponentConfig>,
        config: &EspforgeConfiguration,
    ) -> (Vec<String>, NibblerStatus) {
        let mut findings = Vec::new();
        let mut has_errors = false;

        for (comp_name, comp_config) in components {
            findings.push(format!(
                "Checking component '{}' (using {})",
                comp_name, comp_config.using
            ));

            has_errors |=
                self.validate_component_references(comp_name, comp_config, config, &mut findings);
        }

        let status = if has_errors {
            NibblerStatus::Error
        } else {
            NibblerStatus::Ok
        };

        (findings, status)
    }

    fn validate_component_references(
        &self,
        comp_name: &str,
        comp_config: &crate::config::ComponentConfig,
        config: &EspforgeConfiguration,
        findings: &mut Vec<String>,
    ) -> bool {
        let mut has_errors = false;

        for (key, value) in &comp_config.with {
            if let Some(ref_name) = self.extract_reference(value) {
                if self.is_valid_reference(ref_name, config) {
                    findings.push(format!("  Validated reference '${}'", ref_name));
                } else {
                    findings.push(format!(
                        "  Error: Component '{}' references undefined hardware resource '${}' in property '{}'",
                        comp_name, ref_name, key
                    ));
                    has_errors = true;
                }
            }
        }

        has_errors
    }

    fn extract_reference<'a>(&self, value: &'a Value) -> Option<&'a str> {
        match value {
            Value::String(s) => s.strip_prefix('$'),
            _ => None,
        }
    }

    fn is_valid_reference(&self, ref_name: &str, config: &EspforgeConfiguration) -> bool {
        config
            .esp32
            .as_ref()
            .map(|esp32| {
                esp32.gpio.contains_key(ref_name)
                    || esp32.spi.contains_key(ref_name)
                    || esp32.i2c.contains_key(ref_name)
                    || esp32.uart.contains_key(ref_name)
            })
            .unwrap_or(false)
    }
}

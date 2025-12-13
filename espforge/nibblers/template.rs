use crate::{
    config::EspforgeConfiguration,
    nibblers::{ConfigNibbler, NibblerResult, NibblerStatus},
    register_nibbler,
};
use espforge_macros::auto_register_nibbler;

#[derive(Default)]
#[auto_register_nibbler]
pub struct TemplateNibbler;

impl ConfigNibbler for TemplateNibbler {
    fn name(&self) -> &str {
        "TemplateNibbler"
    }

    fn priority(&self) -> u8 {
        1
    }

    fn process(&self, config: &EspforgeConfiguration) -> Result<NibblerResult, String> {
        let mut findings = Vec::new();
        let status = NibblerStatus::Ok;

        if let Some(template) = config.get_template() {
            findings.push(format!("Using template: {}", template));

            // Example specific check
            if template == "blink"
                && let Some(ex) = &config.example
                && !ex.example_properties.contains_key("blink_rate_ms")
            {
                findings.push("Note: 'blink_rate_ms' not set. Using template default.".to_string());
            }
        } else {
            findings.push("No example template specified. Using minimal default.".to_string());
        }

        Ok(NibblerResult {
            nibbler_name: self.name().to_string(),
            findings,
            status,
        })
    }
}

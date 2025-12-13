use crate::{
    config::EspforgeConfiguration,
    generate::load_manifests,
    nibblers::{ConfigNibbler, NibblerResult, NibblerStatus},
    register_nibbler,
    resolver::actions::{ActionResolver, ValidationResult},
};
use espforge_macros::auto_register_nibbler;

#[derive(Default)]
#[auto_register_nibbler]
pub struct AppNibbler;

impl ConfigNibbler for AppNibbler {
    fn name(&self) -> &str {
        "AppNibbler"
    }

    fn priority(&self) -> u8 {
        30
    }

    fn process(&self, config: &EspforgeConfiguration) -> Result<NibblerResult, String> {
        let mut findings = Vec::new();
        let mut status = NibblerStatus::Ok;

        // We load manifests here to perform semantic validation (checking if methods exist)
        // In a larger system, manifests might be passed in context, but loading here is acceptable.
        let manifests = load_manifests().map_err(|e| e.to_string())?;
        let resolver = ActionResolver::new();

        if let Some(app) = &config.app {
            let mut validate_block = |action_map: &std::collections::HashMap<
                String,
                serde_yaml_ng::Value,
            >,
                                      scope: &str| {
                for (key, value) in action_map {
                    let result = resolver.validate(key, value, config, &manifests);

                    match result {
                        ValidationResult::Ok(msg) => {
                            findings.push(msg);
                        }
                        ValidationResult::Error(msg) => {
                            findings.push(format!("Error in {}: {}", scope, msg));
                            status = NibblerStatus::Error;
                        }
                        ValidationResult::Warning(msg) => {
                            findings.push(format!("Warning in {}: {}", scope, msg));
                            status = NibblerStatus::Warning;
                        }
                        ValidationResult::Ignored => {
                            findings
                                .push(format!("Warning in {}: Unknown action '{}'", scope, key));
                            status = NibblerStatus::Warning;
                        }
                    }
                }
            };

            for action in &app.setup {
                validate_block(action, "app.setup");
            }
            for action in &app.loop_fn {
                validate_block(action, "app.loop");
            }
        }

        Ok(NibblerResult {
            nibbler_name: self.name().to_string(),
            findings,
            status,
        })
    }
}
//register_nibbler!(AppNibbler);

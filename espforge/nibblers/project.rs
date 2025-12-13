use crate::{
    config::EspforgeConfiguration,
    nibblers::{ConfigNibbler, NibblerResult, NibblerStatus},
    register_nibbler,
};
use espforge_macros::auto_register_nibbler;

#[derive(Default)]
#[auto_register_nibbler]
pub struct ProjectNameNibbler;

impl ConfigNibbler for ProjectNameNibbler {
    fn name(&self) -> &str {
        "ProjectNameNibbler"
    }

    fn priority(&self) -> u8 {
        0
    }

    fn process(&self, config: &EspforgeConfiguration) -> Result<NibblerResult, String> {
        let mut findings = Vec::new();
        let mut status = NibblerStatus::Ok;

        let name = config.get_name();

        if name.contains(' ') {
            findings.push(format!("Error: Project name '{}' contains spaces.", name));
            status = NibblerStatus::Error;
        } else if name.chars().any(|c| !c.is_alphanumeric() && c != '_') {
            findings.push(format!(
                "Warning: Project name '{}' contains special characters.",
                name
            ));
            status = NibblerStatus::Warning;
        } else {
            findings.push(format!("Project name '{}' is valid.", name));
        }

        Ok(NibblerResult {
            nibbler_name: self.name().to_string(),
            findings,
            status,
        })
    }
}
register_nibbler!(ProjectNameNibbler);

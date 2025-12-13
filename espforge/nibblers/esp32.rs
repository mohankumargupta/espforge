use crate::{
    config::EspforgeConfiguration,
    nibblers::{ConfigNibbler, NibblerResult, NibblerStatus},
    register_nibbler,
};
use espforge_macros::auto_register_nibbler;

#[derive(Default)]
#[auto_register_nibbler]
pub struct HardwareNibbler;

impl ConfigNibbler for HardwareNibbler {
    fn name(&self) -> &str {
        "HardwareNibbler"
    }

    fn priority(&self) -> u8 {
        10
    }

    fn process(&self, config: &EspforgeConfiguration) -> Result<NibblerResult, String> {
        let mut findings = Vec::new();
        let mut status = NibblerStatus::Ok;

        if let Some(esp32) = &config.esp32 {
            // Check GPIOs
            for (name, pin_config) in &esp32.gpio {
                if pin_config.pin > 48 {
                    findings.push(format!(
                        "Error: GPIO '{}' uses pin {}, which is out of range.",
                        name, pin_config.pin
                    ));
                    status = NibblerStatus::Error;
                } else {
                    findings.push(format!("GPIO '{}' mapped to pin {}.", name, pin_config.pin));
                }
            }

            // Check SPIs
            for (name, spi_config) in &esp32.spi {
                findings.push(format!(
                    "SPI '{}' configured (SCK:{}, MOSI:{}, MISO:{})",
                    name, spi_config.sck, spi_config.mosi, spi_config.miso
                ));
                // Simple overlap check could go here
            }
        }

        Ok(NibblerResult {
            nibbler_name: self.name().to_string(),
            findings,
            status,
        })
    }
}

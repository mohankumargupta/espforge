use std::collections::HashMap;

pub mod components;
pub mod hardware;

pub use components::{Component, ComponentResource, Device, ResourceRef};
pub use hardware::{
    Esp32Config, GpioPinConfig, GpioRef, I2cConfig, I2cRef, PinDirection, ResolvePeripheral,
    SpiConfig, SpiRef, UartConfig, UartRef,
};
// ============================================================================
// Project Model
// ============================================================================

#[derive(Debug, Default)]
pub struct EspforgeConfiguration {
    pub name: String,
    pub chip: String,
    pub esp32: Option<Esp32Config>,
    pub components: HashMap<String, Component>,
    pub devices: HashMap<String, Device>,
}

impl EspforgeConfiguration {
    pub fn get_name(&self) -> &str {
        if self.name.is_empty() {
            "espforge_project"
        } else {
            &self.name
        }
    }

    pub fn get_chip(&self) -> &str {
        &self.chip
    }

    pub fn resolve<'a, R>(
        &'a self,
        reference: &R,
    ) -> Result<&'a R::Config, hardware::ResolutionError>
    where
        R: ResolvePeripheral<'a>,
    {
        let raw = reference.as_str();
        let name = raw
            .strip_prefix('$')
            .ok_or_else(|| hardware::ResolutionError::InvalidPrefix(raw.to_string()))?;

        let map = R::get_map(self)
            .ok_or_else(|| hardware::ResolutionError::MissingSection(R::section_name()))?;

        map.get(name)
            .ok_or_else(|| hardware::ResolutionError::NotFound {
                name: name.to_string(),
                section: R::section_name(),
                available: map.keys().cloned().collect(),
            })
    }
}

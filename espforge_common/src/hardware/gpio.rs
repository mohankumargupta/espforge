use super::ResolvePeripheral;
use crate::EspforgeConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct GpioPinConfig {
    pub pin: u8,
    pub direction: PinDirection,
    #[serde(default)]
    pub pullup: bool,
    #[serde(default)]
    pub pulldown: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum PinDirection {
    Input,
    Output,
}

pub struct GpioRef<'a>(pub &'a str);

impl<'a> ResolvePeripheral<'a> for GpioRef<'a> {
    type Config = GpioPinConfig;

    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>> {
        Some(&root.esp32.as_ref()?.gpio)
    }

    fn as_str(&self) -> &str { self.0 }
    fn section_name() -> &'static str { "gpio" }
}
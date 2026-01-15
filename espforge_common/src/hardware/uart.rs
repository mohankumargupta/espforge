use super::ResolvePeripheral;
use crate::EspforgeConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UartConfig {
    #[serde(default)]
    pub uart: u8,
    pub tx: u8,
    pub rx: u8,
    #[serde(default = "default_uart_baud")]
    pub baud: u32,
}

fn default_uart_baud() -> u32 { 9600 }

pub struct UartRef<'a>(pub &'a str);

impl<'a> ResolvePeripheral<'a> for UartRef<'a> {
    type Config = UartConfig;

    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>> {
        Some(&root.esp32.as_ref()?.uart)
    }

    fn as_str(&self) -> &str { self.0 }
    fn section_name() -> &'static str { "uart" }
}
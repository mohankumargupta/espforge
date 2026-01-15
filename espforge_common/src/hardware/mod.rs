use crate::EspforgeConfiguration;
use std::collections::HashMap;
use thiserror::Error;

pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;

#[derive(Debug, Error)]
pub enum ResolutionError {
    #[error("Reference '{0}' is invalid: missing '$' prefix")]
    InvalidPrefix(String),
    
    #[error("Configuration section 'esp32.{0}' is missing or empty")]
    MissingSection(&'static str),
    
    #[error("Resource '{name}' not found in 'esp32.{section}'. Available: {available:?}")]
    NotFound {
        name: String,
        section: &'static str,
        available: Vec<String>,
    },
}

pub trait ResolvePeripheral<'a> {
    type Config;

    /// Returns the map containing the configs for this peripheral type
    fn get_map(root: &'a EspforgeConfiguration) -> Option<&'a HashMap<String, Self::Config>>;
    
    fn as_str(&self) -> &str;
    fn section_name() -> &'static str;
}
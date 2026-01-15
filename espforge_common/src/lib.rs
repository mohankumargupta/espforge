use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod hardware;

pub use hardware::ResolvePeripheral;
pub use hardware::gpio::{GpioPinConfig, PinDirection, GpioRef};
pub use hardware::spi::{SpiConfig, SpiRef};
pub use hardware::i2c::{I2cConfig, I2cRef};
pub use hardware::uart::{UartConfig, UartRef};


// ============================================================================
// Project Model
// ============================================================================

#[derive(Debug, Default)]
pub struct EspforgeConfiguration {
    pub name: String,
    pub chip: String,
    pub esp32: Option<Esp32Config>,
    pub components: HashMap<String, Component>,
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

    pub fn resolve<'a, R>(&'a self, reference: &R) -> Result<&'a R::Config, hardware::ResolutionError> 
        where R: ResolvePeripheral<'a> 
    {
        let raw = reference.as_str();
        let name = raw.strip_prefix('$')
            .ok_or_else(|| hardware::ResolutionError::InvalidPrefix(raw.to_string()))?;
        
        let map = R::get_map(self)
            .ok_or_else(|| hardware::ResolutionError::MissingSection(R::section_name()))?;
        
        map.get(name).ok_or_else(|| hardware::ResolutionError::NotFound { 
            name: name.to_string(), 
            section: R::section_name(),
            available: map.keys().cloned().collect()
        })
    }
}

// ============================================================================
// Core Configuration Structures
// ============================================================================

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Esp32Config {
    #[serde(default)]
    pub gpio: HashMap<String, GpioPinConfig>,
    #[serde(default)]
    pub spi: HashMap<String, SpiConfig>,
    #[serde(default)]
    pub i2c: HashMap<String, I2cConfig>,
    #[serde(default)]
    pub uart: HashMap<String, UartConfig>,
}

// ============================================================================
// Component System
// ============================================================================
/* 
pub trait ComponentResource {
    type ResourceRefs<'a>: Iterator<Item = ResourceRef<'a>>
    where
        Self: 'a;
    fn resource_refs(&self) -> Self::ResourceRefs<'_>;
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRef<'a> {
    pub resource_type: &'static str,
    pub reference: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "using", content = "with")]
pub enum Component {
    LED {
        gpio: String,
    },
    Button {
        gpio: String,
        #[serde(default)]
        pull_up: bool,
    },
    SpiDevice {
        spi: String,
        #[serde(default)]
        cs: Option<String>,
    },
    I2cDevice {
        i2c: String,
        #[serde(default)]
        address: u8,
    },
    UartDevice {
        uart: String,
        #[serde(default)]
        baud: Option<u32>,
    },
}

impl ComponentResource for Component {
    type ResourceRefs<'a> = Box<dyn Iterator<Item = ResourceRef<'a>> + 'a>;

    fn resource_refs(&self) -> Self::ResourceRefs<'_> {
        match self {
            Self::LED { gpio } | Self::Button { gpio, .. } => {
                Box::new(std::iter::once(ResourceRef {
                    resource_type: "gpio",
                    reference: gpio,
                }))
            }
            Self::SpiDevice { spi, cs } => {
                let spi_ref = ResourceRef {
                    resource_type: "spi",
                    reference: spi,
                };
                Box::new(
                    std::iter::once(spi_ref).chain(cs.iter().map(|cs_ref| ResourceRef {
                        resource_type: "gpio",
                        reference: cs_ref,
                    })),
                )
            }
            Self::I2cDevice { i2c, .. } => Box::new(std::iter::once(ResourceRef {
                resource_type: "i2c",
                reference: i2c,
            })),
            Self::UartDevice { uart, .. } => Box::new(std::iter::once(ResourceRef {
                resource_type: "uart",
                reference: uart,
            })),
        }
    }
}

*/
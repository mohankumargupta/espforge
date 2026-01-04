use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use anyhow::{Context, Result};
use crate::parse::processor::{SectionProcessor, ProcessorRegistration};

// ============================================================================
// Project Model
// ============================================================================

#[derive(Debug, Default)]
pub struct ProjectModel {
    pub name: String,
    pub chip: String,
    pub esp32: Option<Esp32Config>,
    pub components: HashMap<String, Component>,
}

impl ProjectModel {
    pub fn get_name(&self) -> &str {
        if self.name.is_empty() { "espforge_project" } else { &self.name }
    }
    
    pub fn get_chip(&self) -> &str {
        &self.chip
    }

    pub fn resolve<T>(&self, reference: &str) -> Option<&T>
    where
        Esp32Config: AsRef<HashMap<String, T>>,
    {
        self.esp32.as_ref()?.resolve(reference)
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

// Trait for type-safe resource resolution
pub trait ResourceResolver {
    fn resolve<T>(&self, reference: &str) -> Option<&T>
    where
        Self: AsRef<HashMap<String, T>>;
}

impl ResourceResolver for Esp32Config {
    fn resolve<T>(&self, reference: &str) -> Option<&T>
    where
        Self: AsRef<HashMap<String, T>>,
    {
        reference.strip_prefix('$')
            .and_then(|name| self.as_ref().get(name))
    }
}

// Macro to reduce AsRef boilerplate
macro_rules! impl_as_ref_for_esp32 {
    ($($type:ty => $field:ident),+ $(,)?) => {
        $(
            impl AsRef<HashMap<String, $type>> for Esp32Config {
                fn as_ref(&self) -> &HashMap<String, $type> {
                    &self.$field
                }
            }
        )+
    };
}

impl_as_ref_for_esp32! {
    GpioPinConfig => gpio,
    SpiConfig => spi,
    I2cConfig => i2c,
    UartConfig => uart,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UartConfig {
    #[serde(default)]
    pub uart: u8,
    pub tx: u8,
    pub rx: u8,
    #[serde(default = "default_uart_baud")]
    pub baud: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SpiConfig {
    #[serde(default)]
    pub spi: u8,
    #[serde(default)]
    pub miso: Option<u8>,
    pub mosi: u8,
    pub sck: u8,
    #[serde(default)]
    pub cs: Option<u8>,
    #[serde(default = "default_spi_frequency", alias = "frequency_kHz", alias = "frequency_khz")]
    pub frequency: u32,
    #[serde(default)]
    pub mode: u8,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct I2cConfig {
    #[serde(default)]
    pub i2c: u8,
    pub sda: u8,
    pub scl: u8,
    #[serde(default = "default_i2c_frequency", alias = "frequency_kHz", alias = "frequency_khz")]
    pub frequency: u32,
}

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

fn default_i2c_frequency() -> u32 { 100 }
fn default_spi_frequency() -> u32 { 1000 }
fn default_uart_baud() -> u32 { 9600 }

// ============================================================================
// Component System
// ============================================================================

pub trait ComponentResource {
    type ResourceRefs<'a>: Iterator<Item = ResourceRef<'a>> where Self: 'a;
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
    LED { gpio: String },
    Button { gpio: String, #[serde(default)] pull_up: bool },
    SpiDevice { spi: String, #[serde(default)] cs: Option<String> },
    I2cDevice { i2c: String, address: u8 },
    UartDevice { uart: String },
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
                let spi_ref = ResourceRef { resource_type: "spi", reference: spi };
                Box::new(
                    std::iter::once(spi_ref).chain(
                        cs.iter().map(|cs_ref| ResourceRef {
                            resource_type: "gpio",
                            reference: cs_ref,
                        })
                    )
                )
            }
            Self::I2cDevice { i2c, .. } => {
                Box::new(std::iter::once(ResourceRef {
                    resource_type: "i2c",
                    reference: i2c,
                }))
            }
            Self::UartDevice { uart } => {
                Box::new(std::iter::once(ResourceRef {
                    resource_type: "uart",
                    reference: uart,
                }))
            }
        }
    }
}

// ============================================================================
// Main Config Processor (Metadata)
// ============================================================================

#[derive(Deserialize)]
struct MainConfig {
    name: String,
    #[serde(alias = "platform")]
    chip: String,
}

pub struct MainConfigProvisioner;

impl SectionProcessor for MainConfigProvisioner {
    fn section_key(&self) -> &'static str { "espforge" }
    fn priority(&self) -> u32 { 1000 } // Highest priority
    
    fn process(&self, content: &Value, model: &mut ProjectModel) -> Result<()> {
        let config: MainConfig = serde_yaml_ng::from_value(content.clone())
            .context("Failed to parse espforge section")?;
        
        model.name = config.name;
        model.chip = config.chip;
        Ok(())
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(MainConfigProvisioner),
    }
}


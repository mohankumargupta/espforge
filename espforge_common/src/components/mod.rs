use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct ResourceRef<'a> {
    pub resource_type: &'static str,
    pub reference: &'a str,
}

pub trait ComponentResource {
    type ResourceRefs<'a>: Iterator<Item = ResourceRef<'a>>
    where
        Self: 'a;
    fn resource_refs(&self) -> Self::ResourceRefs<'_>;
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "using", content = "with")]
pub enum Component {
    LED {
        gpio: String,
    },
    Button {
        gpio: String,
        #[serde(default)]
        pull_up: Option<bool>,
    },

    #[serde(alias = "spi", alias = "Spi", alias = "SPI")]
    SpiDevice {
        spi: String,
        #[serde(default)]
        cs: Option<String>,
    },

    #[serde(alias = "i2c", alias = "I2c", alias = "I2C")]
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
                if let Some(cs_ref) = cs {
                    Box::new(std::iter::once(spi_ref).chain(std::iter::once(ResourceRef {
                        resource_type: "gpio",
                        reference: cs_ref,
                    })))
                } else {
                    Box::new(std::iter::once(spi_ref))
                }
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

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "using", content = "with")]
pub enum Device {
    #[serde(rename = "ssd1306")]
    SSD1306 {
        component: String,
        #[serde(default = "default_ssd1306_addr")]
        address: u8,
        #[serde(default = "default_ssd1306_width")]
        width: u16,
        #[serde(default = "default_ssd1306_height")]
        height: u16,
    },
    #[serde(rename = "ili9341")]
    ILI9341 {
        spi: String,
        dc: String,
        rst: String,
        cs: String,
    },
}

fn default_ssd1306_addr() -> u8 {
    0x3C
}
fn default_ssd1306_width() -> u16 {
    128
}
fn default_ssd1306_height() -> u16 {
    64
}

impl ComponentResource for Device {
    type ResourceRefs<'a> = Box<dyn Iterator<Item = ResourceRef<'a>> + 'a>;

    fn resource_refs(&self) -> Self::ResourceRefs<'_> {
        match self {
            Self::SSD1306 { component: _, .. } => Box::new(std::iter::empty()),
            Self::ILI9341 {
                spi: _,
                dc,
                rst,
                cs,
            } => Box::new(
                vec![
                    ResourceRef {
                        resource_type: "gpio",
                        reference: dc,
                    },
                    ResourceRef {
                        resource_type: "gpio",
                        reference: rst,
                    },
                    ResourceRef {
                        resource_type: "gpio",
                        reference: cs,
                    },
                ]
                .into_iter(),
            ),
        }
    }
}

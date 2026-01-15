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
                if let Some(cs_ref) = cs {
                    Box::new(
                        std::iter::once(spi_ref).chain(std::iter::once(ResourceRef {
                            resource_type: "gpio",
                            reference: cs_ref,
                        })),
                    )
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
use crate::generator::utils;
use anyhow::Result;
use espforge_common::{Device, EspforgeConfiguration};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn generate_device_registry(model: &EspforgeConfiguration) -> Result<TokenStream> {
    let mut fields = vec![quote! { _marker: PhantomData<&'a ()> }];
    let mut init_logic = Vec::new();
    let mut struct_init = vec![quote! { _marker: PhantomData }];

    let mut sorted_devices: Vec<_> = model.devices.iter().collect();
    sorted_devices.sort_by_key(|(name, _)| *name);

    for (name, device) in sorted_devices {
        let field_name = format_ident!("{}", name);

        match device {
            Device::SSD1306 { component, .. } => {
                let comp_ref = utils::resolve_resource_ident(component)?;
                fields.push(quote! {
                pub #field_name: espforge_devices::devices::ssd1306::device::SSD1306Device<espforge_platform::bus::I2cDevice<'a>>
                });

                init_logic.push(quote! {
                    let #field_name = {
                        let bus_wrapper = espforge_platform::bus::I2cDevice::new(components.#comp_ref.bus());
                        espforge_devices::devices::ssd1306::device::SSD1306Device::new(bus_wrapper)
                    };
                });
            }
            Device::ILI9341 { spi, dc, rst, cs } => {
                let spi_ref = utils::resolve_resource_ident(spi)?;
                let dc_ref = utils::resolve_resource_ident(dc)?;
                let rst_ref = utils::resolve_resource_ident(rst)?;
                let cs_ref = utils::resolve_resource_ident(cs)?;

                fields.push(quote! {
                    pub #field_name: espforge_devices::devices::ili9341::device::ILI9341Device<
                        espforge_platform::bus::SpiDevice<'a>,
                        Output<'static>,
                        Output<'static>
                    >
                });

                init_logic.push(quote! {
                    let #field_name = {
                        // Re-create Output instances from registry pins. 
                        // Platform SpiDevice expects raw Output, not GPIOOutput wrapper.
                        let cs_raw = Output::new(
                            registry.#cs_ref.borrow_mut().take().expect("CS Pin claimed"),
                            Level::High,
                            OutputConfig::default()
                        );
                        let spi_dev = espforge_platform::bus::SpiDevice::new(components.#spi_ref.bus(), cs_raw);
                        let dc_pin = Output::new(
                            registry.#dc_ref.borrow_mut().take().expect("DC Pin claimed"),
                            Level::Low,
                            OutputConfig::default()
                        );
                        let rst_pin = Output::new(
                            registry.#rst_ref.borrow_mut().take().expect("RST Pin claimed"),
                            Level::High,
                            OutputConfig::default()
                        );

                        espforge_devices::devices::ili9341::device::ILI9341Device::new(
                            spi_dev,
                            dc_pin,
                            rst_pin,
                            delay
                        )
                    };
                });
            }
        }

        struct_init.push(quote! { #field_name });
    }

    Ok(quote! {
        pub struct Devices<'a> {
            #(#fields),*
        }

        impl<'a> Devices<'a> {
            pub fn new(
                components: &Components<'a>,
                registry: &'a PeripheralRegistry,
                delay: &mut espforge_platform::delay::Delay
            ) -> Self {
                #(#init_logic)*
                Self { #(#struct_init),* }
            }
        }
    })
}

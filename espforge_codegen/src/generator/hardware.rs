use crate::builders;
use anyhow::Result;
use espforge_configuration::EspforgeConfiguration;
use proc_macro2::TokenStream;
use quote::quote;

pub fn generate_peripheral_registry(model: &EspforgeConfiguration) -> Result<TokenStream> {
    let mut fields = Vec::new();
    let mut init_logic = Vec::new();
    let mut struct_init = Vec::new();

    if model.is_embassy() {
        fields.push(quote! {
            pub sw_interrupt: core::cell::RefCell<Option<esp_hal::peripherals::SW_INTERRUPT<'static>>>
        });
        struct_init.push(quote! {
            sw_interrupt: core::cell::RefCell::new(Some(p.SW_INTERRUPT))
        });
        fields.push(quote! {
            pub timg0: core::cell::RefCell<Option<esp_hal::peripherals::TIMG0<'static>>>
        });
        struct_init.push(quote! {
            timg0: core::cell::RefCell::new(Some(p.TIMG0))
        });
    }

    if let Some(esp32) = &model.esp32 {
        builders::gpio::generate_gpio_pins(&esp32.gpio, &mut fields, &mut struct_init)?;
        builders::i2c::generate_i2c_buses(
            &esp32.i2c,
            &mut fields,
            &mut init_logic,
            &mut struct_init,
        )?;
        builders::spi::generate_spi_buses(
            &esp32.spi,
            &mut fields,
            &mut init_logic,
            &mut struct_init,
        )?;
    }

    Ok(quote! {
        pub struct PeripheralRegistry {
            #(#fields),*
        }

        impl PeripheralRegistry {
            pub fn new(mut p: espforge_platform::esp_hal::peripherals::Peripherals) -> Self {
                #(#init_logic)*

                Self {
                    #(#struct_init),*
                }
            }
        }
    })
}

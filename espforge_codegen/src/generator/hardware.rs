use proc_macro2::TokenStream;
use quote::quote;
use anyhow::Result;
use espforge_common::ProjectModel;
use crate::builders;

pub fn generate_peripheral_registry(model: &ProjectModel) -> Result<TokenStream> {
    let mut fields = Vec::new();
    let mut init_logic = Vec::new();
    let mut struct_init = Vec::new();

    if let Some(esp32) = &model.esp32 {
        builders::spi::generate_spi_buses(
            &esp32.spi,
            &mut fields,
            &mut init_logic,
            &mut struct_init,
        )?;

        builders::i2c::generate_i2c_buses(
            &esp32.i2c,
            &mut fields,
            &mut init_logic,
            &mut struct_init,
        )?;

        builders::gpio::generate_gpio_pins(
            &esp32.gpio,
            &mut fields,
            &mut struct_init,
        )?;
    }

    Ok(quote! {
        /// Owns the raw hardware peripherals and buses
        pub struct PeripheralRegistry {
            #(#fields),*
        }

        impl PeripheralRegistry {
            pub fn new(p: Peripherals) -> Self {
                #(#init_logic)*
                Self { #(#struct_init),* }
            }
        }
    })
}
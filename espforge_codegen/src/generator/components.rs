use proc_macro2::TokenStream;
use quote::quote;
use anyhow::Result;
use espforge_common::{EspforgeConfiguration, Component};
use crate::component_builders;

pub fn generate_component_registry(model: &EspforgeConfiguration) -> Result<TokenStream> {
    let mut fields = vec![quote! { _marker: PhantomData<&'a ()> }];
    let mut init_logic = Vec::new();
    let mut struct_init = vec![quote! { _marker: PhantomData }];

    let mut sorted_components: Vec<_> = model.components.iter().collect();
    sorted_components.sort_by_key(|(name, _)| *name);

    for (name, component) in sorted_components {
        match component {
            Component::LED { gpio } => {
                component_builders::led::generate(name, gpio, &mut fields, &mut init_logic, &mut struct_init)?;
            }
            Component::Button { gpio, pull_up } => {
                component_builders::button::generate(name, gpio, *pull_up, &mut fields, &mut init_logic, &mut struct_init)?;
            }
            Component::SpiDevice { spi, cs } => {
                component_builders::spi_device::generate(name, spi, cs.as_deref(), &mut fields, &mut init_logic, &mut struct_init)?;
            }
            Component::I2cDevice { i2c, address } => {
                component_builders::i2c_device::generate(name, i2c, *address, &mut fields, &mut init_logic, &mut struct_init)?;
            }
            Component::UartDevice { uart, baud } => {
                component_builders::uart_device::generate(name, uart, *baud, model, &mut fields, &mut init_logic, &mut struct_init)?;
            }
        }
    }

    Ok(quote! {
        pub struct Components<'a> {
            #(#fields),*
        }

        impl<'a> Components<'a> {
            pub fn new(registry: &'a PeripheralRegistry) -> Self {
                #(#init_logic)*
                Self { #(#struct_init),* }
            }
        }
    })
}

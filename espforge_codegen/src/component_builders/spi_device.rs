use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::Result;
use crate::generator::utils;

pub fn generate(
    name: &str,
    spi: &str,
    cs: Option<&str>,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    let field = format_ident!("{}", name);
    let spi_ref = utils::resolve_resource_ident(spi)?;

    if let Some(cs_ref_str) = cs {
        let cs_ref = utils::resolve_resource_ident(cs_ref_str)?;

        fields.push(quote! { pub #field: platform::bus::SpiDevice<'a> });

        init_logic.push(quote! {
            let #field = platform::bus::SpiDevice::new(
                &registry.#spi_ref,
                platform::gpio::GPIOOutput::from_pin(
                    registry.#cs_ref.borrow_mut().take().expect("CS Pin already claimed")
                )
            );
        });
    } else {
        fields.push(quote! { pub #field: platform::components::spi::SPI<'a> });
        init_logic.push(quote! {
            let #field = platform::components::spi::SPI::new(&registry.#spi_ref);
        });
    }

    struct_init.push(quote! { #field });
    Ok(())
}


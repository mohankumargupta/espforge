use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::Result;
use crate::generator::utils;

pub fn generate(
    name: &str,
    i2c: &str,
    _address: u8, // Could be used for validation
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    let field = format_ident!("{}", name);
    let i2c_ref = utils::resolve_resource_ident(i2c)?;

    fields.push(quote! { pub #field: platform::bus::I2cDevice<'a> });
    
    init_logic.push(quote! {
       let #field = platform::bus::I2cDevice::new(&registry.#i2c_ref);
    });
    
    struct_init.push(quote! { #field });
    Ok(())
}

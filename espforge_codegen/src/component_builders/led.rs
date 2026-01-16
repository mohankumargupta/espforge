use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::Result;
use crate::generator::utils;

pub fn generate(
    name: &str,
    gpio: &str,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    let field = format_ident!("{}", name);
    let pin_ref = utils::resolve_resource_ident(gpio)?;

    fields.push(quote! { pub #field: Output<'static> });
    
    init_logic.push(quote! {
        let #field = Output::new(
            registry.#pin_ref.borrow_mut().take().expect("Pin already claimed"),
            Level::Low,
            OutputConfig::default()
        );
    });
    
    struct_init.push(quote! { #field });
    Ok(())
}


use crate::generator::utils;
use anyhow::Result;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn generate(
    name: &str,
    gpio: &str,
    pull_up: Option<bool>,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    let field = format_ident!("{}", name);
    let pin_ref = utils::resolve_resource_ident(gpio)?;

    let pull = if pull_up.unwrap_or(false) {
        quote! { Pull::Up }
    } else {
        quote! { Pull::None }
    };

    // Use raw HAL Input type instead of opaque wrapper
    fields.push(quote! { pub #field: Input<'static> });

    init_logic.push(quote! {
        let #field = Input::new(
            registry.#pin_ref.borrow_mut().take().expect("Pin already claimed"),
            InputConfig::default().with_pull(#pull)
        );
        });

    struct_init.push(quote! { #field });
    Ok(())
}

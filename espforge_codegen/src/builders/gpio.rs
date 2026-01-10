use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::Result;
use std::collections::HashMap;
use crate::parse::model::GpioConfig;

pub fn generate_gpio_pins(
    gpio_configs: &HashMap<String, GpioConfig>,
    fields: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    for (name, cfg) in gpio_configs {
        let field = format_ident!("{}", name);
        let pin_num = format_ident!("GPIO{}", cfg.pin);

        fields.push(quote! { pub #field: RefCell<Option<AnyPin<'static>>> });
        struct_init.push(quote! { #field: RefCell::new(Some(p.#pin_num.degrade())) });
    }
    Ok(())
}

use espforge_common::I2cConfig;
use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::Result;
use std::collections::HashMap;

pub fn generate_i2c_buses(
    i2c_configs: &HashMap<String, I2cConfig>,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    for (name, cfg) in i2c_configs {
        let field = format_ident!("{}", name);
        let i2c_peri = format_ident!("I2C{}", cfg.i2c);
        let sda = format_ident!("GPIO{}", cfg.sda);
        let scl = format_ident!("GPIO{}", cfg.scl);
        let freq = cfg.frequency;

        fields.push(quote! { pub #field: RefCell<I2c<'static, Blocking>> });

        init_logic.push(quote! {
            let #field = I2c::new(
                    p.#i2c_peri, 
                    esp_hal::i2c::master::Config::default()
                        .with_frequency(esp_hal::time::Rate::from_khz(#freq))
                ).unwrap()
                .with_sda(p.#sda.degrade())
                .with_scl(p.#scl.degrade());
        });

        struct_init.push(quote! { #field: RefCell::new(#field) });
    }
    Ok(())
}

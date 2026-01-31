use anyhow::Result;
use espforge_configuration::hardware::i2c::I2cConfig;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashMap;

pub fn generate_i2c_buses(
    configs: &HashMap<String, I2cConfig>,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    for (name, cfg) in configs {
        let field = format_ident!("{}", name);
        let i2c_peri = format_ident!("I2C{}", cfg.i2c);
        let sda = format_ident!("GPIO{}", cfg.sda);
        let scl = format_ident!("GPIO{}", cfg.scl);

        let freq = cfg.frequency;

        // Add field to struct
        fields.push(quote! { pub #field: RefCell<I2c<'static, Blocking>> });

        // Add initialization logic
        init_logic.push(quote! {
            let #field = espforge_platform::esp_hal::i2c::master::I2c::new(
                p.#i2c_peri,
                espforge_platform::esp_hal::i2c::master::Config::default()
                    .with_frequency(
                        espforge_platform::esp_hal::time::Rate::from_khz(#freq)
                    )
            )
            .expect("Failed to initialize I2C")
            .with_sda(p.#sda)
            .with_scl(p.#scl);
        });

        // Add to struct init
        struct_init.push(quote! { #field: RefCell::new(#field) });
    }

    Ok(())
}
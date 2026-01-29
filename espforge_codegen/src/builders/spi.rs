use anyhow::Result;
use espforge_common::hardware::spi::SpiConfig;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashMap;

pub fn generate_spi_buses(
    configs: &HashMap<String, SpiConfig>,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    for (name, cfg) in configs {
        let field = format_ident!("{}", name);
        let spi_peri = format_ident!("SPI{}", cfg.spi);
        let sck = format_ident!("GPIO{}", cfg.sck);
        let mosi = format_ident!("GPIO{}", cfg.mosi);
        let freq = cfg.frequency;
        let miso = cfg.miso.map(|pin| format_ident!("GPIO{}", pin));
        let cs = cfg.cs.map(|pin| format_ident!("GPIO{}", pin));

        // Add field to struct
        fields.push(quote! { pub #field: RefCell<Spi<'static, Blocking>> });

        // Add initialization logic
        let mut spi_chain = quote! {
            let mut #field = espforge_platform::esp_hal::spi::master::Spi::new(
                p.#spi_peri,
                espforge_platform::esp_hal::spi::master::Config::default()
                    .with_frequency(
                        espforge_platform::esp_hal::time::Rate::from_khz(#freq)
                    )
                    .with_mode(espforge_platform::esp_hal::spi::Mode::_0) // TODO: Make configurable
            )
            .expect("Failed to initialize SPI")
            .with_sck(p.#sck)
            .with_mosi(p.#mosi)
        };

        if let Some(miso_pin) = miso {
            spi_chain.extend(quote! {
                .with_miso(p.#miso_pin)
            });
        }

        if let Some(cs_pin) = cs {
            spi_chain.extend(quote! {
                .with_cs(p.#cs_pin)
            });
        }

        // Add semicolon to complete the statement
        spi_chain.extend(quote! { ; });

        init_logic.push(spi_chain);

        // Add to struct init
        struct_init.push(quote! { #field: RefCell::new(#field) });
    }

    Ok(())
}

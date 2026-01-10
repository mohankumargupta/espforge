use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::Result;
use std::collections::HashMap;
use crate::parse::model::SpiConfig;

pub fn generate_spi_buses(
    spi_configs: &HashMap<String, SpiConfig>,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    for (name, cfg) in spi_configs {
        let field = format_ident!("{}", name);
        let spi_peri = format_ident!("SPI{}", cfg.spi);
        let sck = format_ident!("GPIO{}", cfg.sck);
        let mosi = format_ident!("GPIO{}", cfg.mosi);
        let freq = cfg.frequency;

        fields.push(quote! { pub #field: RefCell<Spi<'static, Blocking>> });

        let miso_cfg = cfg.miso.map(|m| {
            let m_pin = format_ident!("GPIO{}", m);
            quote! { .with_miso(p.#m_pin.degrade()) }
        }).unwrap_or_else(|| quote! {});

        let cs_cfg = cfg.cs.map(|c| {
            let c_pin = format_ident!("GPIO{}", c);
            quote! { .with_cs(p.#c_pin.degrade()) }
        }).unwrap_or_else(|| quote! {});

        init_logic.push(quote! {
            let #field = Spi::new(
                    p.#spi_peri, 
                    esp_hal::spi::master::Config::default()
                        .with_frequency(esp_hal::time::Rate::from_khz(#freq))
                        .with_mode(Mode::_0)
                ).unwrap()
                .with_sck(p.#sck.degrade())
                .with_mosi(p.#mosi.degrade())
                #miso_cfg
                #cs_cfg;
        });

        struct_init.push(quote! { #field: RefCell::new(#field) });
    }
    Ok(())
}

use anyhow::Result;
use espforge_configuration::hardware::psram::{PsramConfig, PsramMode};
use proc_macro2::TokenStream;
use quote::quote;

pub fn generate_psram_init(config: &PsramConfig) -> Result<TokenStream> {
    let mode = match config.mode {
        PsramMode::Quad => quote! { esp_hal::psram::PsramMode::Quad },
        PsramMode::Octal => quote! { esp_hal::psram::PsramMode::Octal },
        PsramMode::Hex => quote! { esp_hal::psram::PsramMode::Hex },
    };

    Ok(quote! {
        esp_hal::psram::init_psram(unsafe { esp_hal::peripherals::PSRAM::steal() }, #mode);
    })
}
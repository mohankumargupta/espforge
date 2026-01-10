use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use anyhow::{Result, anyhow};
use crate::parse::model::ProjectModel;

pub fn generate(
    name: &str,
    uart: &str,
    baud: Option<u32>,
    model: &ProjectModel,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    let field = format_ident!("{}", name);
    let uart_ref = uart.strip_prefix('$').unwrap_or(uart);
    
    let esp32 = model.esp32.as_ref()
        .ok_or_else(|| anyhow!("No ESP32 configuration found"))?;
    
    let cfg = esp32.uart.get(uart_ref)
        .ok_or_else(|| anyhow!("UART resource {} not found", uart))?;

    let uart_num = cfg.uart;
    let tx_pin = cfg.tx;
    let rx_pin = cfg.rx;
    let baud_rate = baud.unwrap_or(cfg.baud);

    fields.push(quote! { pub #field: platform::components::uart::Uart });
    
    init_logic.push(quote! {
        let #field = platform::components::uart::Uart::new(
            #uart_num,
            #tx_pin,
            #rx_pin,
            #baud_rate
        );
    });
    
    struct_init.push(quote! { #field });
    Ok(())
}


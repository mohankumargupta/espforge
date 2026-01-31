use anyhow::Result;
use espforge_configuration::EspforgeConfiguration;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn generate(
    name: &str,
    properties: &serde_yaml_ng::Value,
    _model: &EspforgeConfiguration,
    fields: &mut Vec<TokenStream>,
    init_logic: &mut Vec<TokenStream>,
    struct_init: &mut Vec<TokenStream>,
) -> Result<()> {
    let config: espforge_common::components::i2c::I2cDeviceConfig =
        serde_yaml_ng::from_value(properties.clone())?;

    let field = format_ident!("{}", name);
    let i2c_name = config.i2c.strip_prefix('$').unwrap_or(&config.i2c);
    let bus_ident = format_ident!("{}", i2c_name);

    fields.push(quote! { pub #field: espforge_components::components::i2c::I2C<'a> });

    init_logic.push(quote! {
        let #field = espforge_components::components::i2c::I2C::new(&#bus_ident);
    });

    struct_init.push(quote! { #field });

    Ok(())
}

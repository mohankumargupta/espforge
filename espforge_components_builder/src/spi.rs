use anyhow::{Context, Result};
use espforge_common::components::spi::SpiDeviceConfig;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext, Plugin, PluginKind, PluginRegistration};
use quote::{format_ident, quote};
use serde_yaml_ng;

pub struct SpiDevicePlugin;

impl Plugin for SpiDevicePlugin {
    fn name(&self) -> &'static str { "SpiDevice" }
    fn kind(&self) -> PluginKind { PluginKind::Component }
    
    fn validate(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: SpiDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }

    fn dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: SpiDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        let spi_name = config.spi.strip_prefix('$').unwrap_or(&config.spi);
        Ok(vec![Dependency::peripheral(spi_name)])
    }

    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: SpiDeviceConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid SpiDevice configuration")?;
        let field_ident = format_ident!("{}", ctx.instance_name);
        
        let spi_name = config.spi.strip_prefix('$').unwrap_or(&config.spi);
        let spi_ident = format_ident!("{}", spi_name);

        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::spi::SPI<'a> },
            init: quote! {
                let #field_ident = espforge_components::components::spi::SPI::new(&registry.#spi_ident);
            },
            struct_init: quote! { #field_ident },
        })
    }
}

inventory::submit! {
    PluginRegistration(&SpiDevicePlugin)
}
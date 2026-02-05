use espforge_common::components::spi::SpiDeviceConfig;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;
use anyhow::{Result, Context};
use quote::{quote, format_ident};
use std::str::FromStr;

#[derive(ComponentPlugin)]
#[plugin(name = "SpiDevice")]
pub struct SpiDevicePlugin;

impl SpiDevicePlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: SpiDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }
    
    fn resolve_dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: SpiDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        let spi_name = config.spi.strip_prefix('$').unwrap_or(&config.spi);
        Ok(vec![Dependency::peripheral(spi_name)])
    }
    
    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: SpiDeviceConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid SpiDevice configuration")?;

        let field_ident = format_ident!("{}", ctx.instance_name);
        let spi_name = config.spi.strip_prefix('$').unwrap_or(&config.spi);

        let dep = ctx.resolved_deps.get(spi_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Dependency '{}' not found for component '{}'",
                spi_name,
                ctx.instance_name
            )
        })?;

        let bus_access = proc_macro2::TokenStream::from_str(&dep.access_path)
            .map_err(|e| anyhow::anyhow!("Failed to parse access path: {}", e))?;

        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::spi::SPI },
            init: quote! {
                let #field_ident = espforge_components::components::spi::SPI::new(&#bus_access);
            },
            struct_init: quote! { #field_ident },
        })
    }
}

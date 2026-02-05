use espforge_common::components::i2c::I2cDeviceConfig;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;
use anyhow::{Result, Context};
use quote::{quote, format_ident};
use std::str::FromStr;

#[derive(ComponentPlugin)]
#[plugin(name = "I2cDevice")]
pub struct I2cDevicePlugin;

impl I2cDevicePlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: I2cDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }
    
    fn resolve_dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: I2cDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        let i2c_name = config.i2c.strip_prefix('$').unwrap_or(&config.i2c);
        Ok(vec![Dependency::peripheral(i2c_name)])
    }
    
    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: I2cDeviceConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid I2cDevice configuration")?;

        let field_ident = format_ident!("{}", ctx.instance_name);
        let i2c_name = config.i2c.strip_prefix('$').unwrap_or(&config.i2c);

        let dep = ctx.resolved_deps.get(i2c_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Dependency '{}' not found for component '{}'",
                i2c_name,
                ctx.instance_name
            )
        })?;

        let bus_access = proc_macro2::TokenStream::from_str(&dep.access_path)
            .map_err(|e| anyhow::anyhow!("Failed to parse access path: {}", e))?;

        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::i2c::I2C },
            init: quote! {
                let #field_ident = espforge_components::components::i2c::I2C::new(&#bus_access);
            },
            struct_init: quote! { #field_ident },
        })
    }
}

use espforge_common::components::i2c::I2cDeviceConfig;
use espforge_common::dependency::Dependency;
use espforge_common::plugin::{GeneratedCode, GenerationContext, Plugin, PluginKind, PluginRegistration};
use anyhow::{Context, Result};
use quote::{format_ident, quote};
use serde_yaml_ng;
use std::str::FromStr;

pub struct I2cDevicePlugin;

impl Plugin for I2cDevicePlugin {
    fn name(&self) -> &'static str {
        "I2cDevice"
    }

    fn kind(&self) -> PluginKind {
        PluginKind::Component
    }

    fn validate(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: I2cDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }

    fn dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: I2cDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        let i2c_name = config.i2c.strip_prefix('$').unwrap_or(&config.i2c);
        Ok(vec![Dependency::peripheral(i2c_name)])
    }

    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: I2cDeviceConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid I2cDevice configuration")?;
        
        let field_ident = format_ident!("{}", ctx.instance_name);
        let i2c_name = config.i2c.strip_prefix('$').unwrap_or(&config.i2c);

        // Retrieve the resolved dependency to get the correct access path (e.g., "registry.i2c0")
        let dep = ctx.resolved_deps.get(i2c_name)
            .ok_or_else(|| anyhow::anyhow!("Dependency '{}' not found for component '{}'", i2c_name, ctx.instance_name))?;
        
        // Parse the access string (e.g., "registry.i2c0") into a TokenStream
        let bus_access = proc_macro2::TokenStream::from_str(&dep.access_path)
            .map_err(|e| anyhow::anyhow!("Failed to parse access path: {}", e))?;

        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::i2c::I2C<'a> },
            init: quote! {
                let #field_ident = espforge_components::components::i2c::I2C::new(&#bus_access);
            },
            struct_init: quote! { #field_ident },
        })
    }
}

inventory::submit! {
    PluginRegistration(&I2cDevicePlugin)
}
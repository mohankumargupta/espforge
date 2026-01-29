use anyhow::{Context, Result};
use espforge_common::components::led::LedConfig;
use espforge_common::dependency::Dependency;
use espforge_common::plugin::{GeneratedCode, GenerationContext, Plugin, PluginKind, PluginRegistration};
use quote::{format_ident, quote};
use serde_yaml_ng;

pub struct LedPlugin;

impl Plugin for LedPlugin {
    fn name(&self) -> &'static str { "LED" }
    fn kind(&self) -> PluginKind { PluginKind::Component }
    
    fn validate(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: LedConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }

    fn dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: LedConfig = serde_yaml_ng::from_value(properties.clone())?;
        let pin_name = config.gpio.strip_prefix('$').unwrap_or(&config.gpio);
        Ok(vec![Dependency::pin(pin_name)])
    }

    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: LedConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid LED configuration")?;
        let field_ident = format_ident!("{}", ctx.instance_name);
        
        let pin_name = config.gpio.strip_prefix('$').unwrap_or(&config.gpio);
        let pin_ident = format_ident!("{}", pin_name);

        let active_low = config.active_low;

        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::led::component::LED },
            init: quote! {
                let #field_ident = espforge_components::components::led::component::LED::new(
                    espforge_platform::gpio::GPIOOutput::from_pin(registry.#pin_ident.borrow_mut().take().unwrap()),
                    espforge_components::components::led::component::LedConfig {
                        active_low: #active_low,
                        ..Default::default()
                    }
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}

inventory::submit! {
    PluginRegistration(&LedPlugin)
}
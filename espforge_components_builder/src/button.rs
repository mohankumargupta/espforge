use anyhow::{Context, Result};
use espforge_common::components::button::ButtonConfig;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(name = "Button")]
pub struct ButtonPlugin;

impl ButtonPlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: ButtonConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }

    fn resolve_dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: ButtonConfig = serde_yaml_ng::from_value(properties.clone())?;
        let pin_name = config.gpio.strip_prefix('$').unwrap_or(&config.gpio);
        Ok(vec![Dependency::pin(pin_name)])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: ButtonConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid Button configuration")?;
        let field_ident = format_ident!("{}", ctx.instance_name);
        let pin_name = config.gpio.strip_prefix('$').unwrap_or(&config.gpio);
        let pin_ident = format_ident!("{}", pin_name);

        let pull_up = config.pull_up;

        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::button::Button },
            init: quote! {
                let #field_ident = espforge_components::components::button::Button::new(
                    espforge_platform::gpio::GPIOInput::from_pin(
                        registry.#pin_ident.borrow_mut().take().unwrap(),
                        #pull_up,
                        false,
                    ),
                    espforge_components::components::button::ButtonConfig {
                        pull_up: #pull_up,
                        ..Default::default()
                    }
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}

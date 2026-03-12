use anyhow::{Result, anyhow};
use espforge_configuration::plugin::{
    ComponentRef, Dependency, DependencyKind, DeviceRef, GeneratedCode, GenerationContext, codegen
};
use espforge_macros::DevicePlugin;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;
//use serde_yaml_ng;


#[derive(Deserialize, Debug, Clone)]
pub struct SSD1306Config {
    pub component: DeviceRef<ComponentRef>,
    pub address: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(DevicePlugin)]
#[plugin(name = "ssd1306", features = "ssd1306", config = "SSD1306Config")]
pub struct SSD1306Plugin;

impl SSD1306Plugin {
    fn validate_config(&self, config: &SSD1306Config) -> Result<()> {
        if let Some(w) = config.width {
            if w == 0 {
                return Err(anyhow!("Display dimensions must be greater than 0"));
            }
        }
        if let Some(addr) = config.address {
            if addr > 0x7F {
                return Err(anyhow!("I2C address must be 7-bit (0x00-0x7F)"));
            }
        }
        Ok(())
    }

    fn resolve_dependencies(&self, config: &SSD1306Config) -> Result<Vec<Dependency>> {
         Ok(vec![Dependency::component(config.component.as_str())])
    }

    fn generate_code(&self, config: &SSD1306Config,ctx: &GenerationContext) -> Result<GeneratedCode> {
        let field_ident = format_ident!("{}", ctx.instance_name);
        let dep_ident: TokenStream = ctx
            .dependency_access(config.component.as_str(), DependencyKind::Component)
            .map_err(|e| {
                anyhow!(
                    "Failed to resolve component '{}' for device '{}': {}",
                    config.component,
                    ctx.instance_name,
                    e
                )
            })?;

        let field = quote! {
                 espforge_devices::SSD1306Device<espforge_components::I2C>
        };
        let init = quote! {
                let mut #field_ident = espforge_devices::SSD1306Device::new(
                    #dep_ident
                );
                #field_ident.init();
                #field_ident
        };

        Ok(codegen(&ctx.instance_name, field, init))

        // Ok(GeneratedCode {
        //     field: quote! {
        //         pub #field_ident: espforge_devices::devices::ssd1306::device::SSD1306Device<espforge_components::components::i2c::I2C>
        //     },
        //     init: quote! {
        //         let mut #field_ident = espforge_devices::devices::ssd1306::device::SSD1306Device::new(
        //             #dep_ident
        //         );
        //         #field_ident.init();
        //     },
        //     struct_init: quote! { #field_ident },
        // })
    }
}

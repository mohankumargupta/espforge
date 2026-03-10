use anyhow::{Context, Result};
use espforge_configuration::plugin::{
    ComponentRef, Dependency, DependencyKind, DeviceRef,  GeneratedCode, GenerationContext,
};
use espforge_macros::DevicePlugin;
use quote::{format_ident, quote};
use serde::Deserialize;


#[derive(Debug, Deserialize)]
pub struct FT6206Config {
    pub component: DeviceRef<ComponentRef>,
    #[serde(default)]
    pub address: Option<u8>,
    #[serde(default)]
    pub mirror_x: bool,
    #[serde(default)]
    pub mirror_y: bool,
    #[serde(default)]
    pub swap_xy: bool,
    #[serde(default)]
    pub screen_width: u16,
    #[serde(default)]
    pub screen_height: u16,
    pub x_min: Option<u16>,
    pub x_max: Option<u16>,
    pub y_min: Option<u16>,
    pub y_max: Option<u16>,
}

#[derive(DevicePlugin)]
#[plugin(name = "ft6206", features = "ft6206")]
pub struct FT6206Plugin;

impl FT6206Plugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: FT6206Config = serde_yaml_ng::from_value(properties.clone())
            .context("Invalid FT6206 configuration")?;
        Ok(())
    }

    fn resolve_dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: FT6206Config = serde_yaml_ng::from_value(properties.clone())?;
        Ok(vec![Dependency::component(config.component.as_str())])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: FT6206Config = serde_yaml_ng::from_value(ctx.properties.clone())?;
        let field_name = format_ident!("{}", ctx.instance_name);

        let address = config.address.unwrap_or(0x38);
        let mirror_x = config.mirror_x;
        let mirror_y = config.mirror_y;
        let swap_xy = config.swap_xy;
        let screen_width = config.screen_width;
        let screen_height = config.screen_height;
        let x_min = config.x_min.unwrap_or(0);
        let x_max = config.x_max.unwrap_or(screen_width);
        let y_min = config.y_min.unwrap_or(0);
        let y_max = config.y_max.unwrap_or(screen_height);

        let i2c_access = ctx.dependency_access(config.component.as_str(), DependencyKind::Component)?;        
        // let dep_ident =
        //     ctx.dependency_access(config.component.as_str(), DependencyKind::Component)?;


        Ok(GeneratedCode {
            // Field definition in struct Context
            field: quote! {
                pub #field_name: espforge_devices::devices::ft6206::device::FT6206<espforge_components::components::i2c::I2C>
            },
            // Initialization logic in main
            init: quote! {
                let mut #field_name = espforge_devices::devices::ft6206::device::FT6206::new(
                    #i2c_access,
                    #address,
                    #mirror_x,
                    #mirror_y,
                    #swap_xy,
                    #screen_width,
                    #screen_height,
                    #x_min,
                    #x_max,
                    #y_min,
                    #y_max,
                );
                #field_name.init().ok();
            },
            // Placing it into Context
            struct_init: quote! { #field_name },
        })
    }
}

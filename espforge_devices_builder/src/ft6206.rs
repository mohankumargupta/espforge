use anyhow::Result;
use espforge_configuration::plugin::{
    ComponentRef, Dependency, DependencyKind, DeviceRef, GeneratedCode, GenerationContext, codegen,
};
use espforge_macros::DevicePlugin;
use quote::quote;
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
#[plugin(name = "ft6206", features = "ft6206", config = "FT6206Config")]
pub struct FT6206Plugin;

impl FT6206Plugin {
    fn validate_config(&self, _config: &FT6206Config) -> Result<()> {
        Ok(())
    }

    fn resolve_dependencies(&self, config: &FT6206Config) -> Result<Vec<Dependency>> {
        Ok(vec![Dependency::component(config.component.as_str())])
    }

    fn generate_code(
        &self,
        config: &FT6206Config,
        ctx: &GenerationContext,
    ) -> Result<GeneratedCode> {
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

        let i2c_access =
            ctx.dependency_access(config.component.as_str(), DependencyKind::Component)?;

        let field = quote! {
            espforge_devices::FT6206<
                espforge_platform::bus::I2cDevice<'static>
            >
        };

        let init = quote! {
                let mut dev = espforge_devices::FT6206::new(
                    #i2c_access,
                    #address,
                    #screen_width,
                    #screen_height,
                    #x_min,
                    #x_max,
                    #y_min,
                    #y_max,
                    #swap_xy,
                    #mirror_x,
                    #mirror_y,
                );
                dev.init().expect("FT6206 init failed");
                dev
        };

        Ok(codegen(&ctx.instance_name, field, init))

        // let dep_ident =
        //     ctx.dependency_access(config.component.as_str(), DependencyKind::Component)?;

        // Ok(GeneratedCode {
        //     // Field definition in struct Context
        //     field: quote! {
        //         pub #field_name: espforge_devices::devices::ft6206::device::FT6206<espforge_components::components::i2c::I2C>
        //     },
        //     // Initialization logic in main
        //     init: quote! {
        //         let mut #field_name = espforge_devices::devices::ft6206::device::FT6206::new(
        //             #i2c_access,
        //             #address,
        //             #mirror_x,
        //             #mirror_y,
        //             #swap_xy,
        //             #screen_width,
        //             #screen_height,
        //             #x_min,
        //             #x_max,
        //             #y_min,
        //             #y_max,
        //         );
        //         #field_name.init().ok();
        //     },
        //     // Placing it into Context
        //     struct_init: quote! { #field_name },
        // })
    }
}

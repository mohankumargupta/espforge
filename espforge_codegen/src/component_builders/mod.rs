use anyhow::Result;
use espforge_configuration::EspforgeConfiguration;
use proc_macro2::TokenStream;

pub mod button;
pub mod i2c_device;
pub mod spi_device;
pub mod uart_device;

pub trait ComponentGenerator {
    fn generate(
        &self,
        name: &str,
        model: &EspforgeConfiguration,
        fields: &mut Vec<TokenStream>,
        init_logic: &mut Vec<TokenStream>,
        struct_init: &mut Vec<TokenStream>,
    ) -> Result<()>;
}

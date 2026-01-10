use espforge_common::ProjectModel;
use proc_macro2::TokenStream;
use anyhow::Result;

pub mod led;
pub mod button;
pub mod spi_device;
pub mod i2c_device;
pub mod uart_device;




pub trait ComponentGenerator {
    fn generate(
        &self,
        name: &str,
        model: &ProjectModel,
        fields: &mut Vec<TokenStream>,
        init_logic: &mut Vec<TokenStream>,
        struct_init: &mut Vec<TokenStream>,
    ) -> Result<()>;
}


use super::common::format_generated_source;
use super::constants::{origins, ALLOW_ATTRS};
use crate::context::CodegenContext;
use anyhow::{Context, Result};
use espforge_configuration::EspforgeConfiguration;
use proc_macro2::TokenStream;
use quote::quote;

/// Generates the components source file (generated.rs)
///
/// # Arguments
/// * `model` - The configuration model containing component and device specifications
///
/// # Returns
/// Formatted Rust source code for generated.rs
pub fn generate_components_source(model: &EspforgeConfiguration) -> Result<String> {
    let builder = ComponentsBuilder::new(model)?;
    let code = builder.build()?;

    Ok(format_generated_source(
        &format!("{}{}", ALLOW_ATTRS, code),
        origins::COMPONENTS,
    ))
}

/// Builder for constructing the components source code
struct ComponentsBuilder<'a> {
    model: &'a EspforgeConfiguration,
    ctx: CodegenContext,
}

impl<'a> ComponentsBuilder<'a> {
    fn new(model: &'a EspforgeConfiguration) -> Result<Self> {
        let ctx = CodegenContext::build(model)
            .context("Failed to build codegen context")?;

        Ok(Self { model, ctx })
    }

    fn build(self) -> Result<String> {
        let tokens = self
            .ctx
            .generate()
            .context("Failed to generate component tokens")?;

        let imports = self.generate_imports();

        let output = quote! {
            #imports
            #tokens
        };

        self.parse_and_format(output)
    }

    fn generate_imports(&self) -> TokenStream {
        let has_spi = self
            .model
            .esp32
            .as_ref()
            .map_or(false, |p| !p.spi.is_empty());

        let has_i2c = self
            .model
            .esp32
            .as_ref()
            .map_or(false, |p| !p.i2c.is_empty());

        ImportBuilder::new()
            .with_base_imports()
            .with_spi_imports_if(has_spi)
            .with_i2c_imports_if(has_i2c, has_spi)
            .build()
    }

    fn parse_and_format(self, output: TokenStream) -> Result<String> {
        // Debug output for development/troubleshooting
        if let Err(e) = syn::parse2::<syn::File>(output.clone()) {
            eprintln!("---------------------------------------------------");
            eprintln!("DEBUG: Generated Code causing syntax error:");
            eprintln!("Error: {}", e);
            eprintln!("{}", output);
            eprintln!("---------------------------------------------------");
            return Err(anyhow::anyhow!("Failed to parse generated tokens: {}", e));
        }

        let syntax_tree = syn::parse2(output)
            .context("Failed to parse generated component tokens")?;

        Ok(prettyplease::unparse(&syntax_tree))
    }
}

/// Builder for constructing import statements
struct ImportBuilder {
    imports: TokenStream,
}

impl ImportBuilder {
    fn new() -> Self {
        Self {
            imports: TokenStream::new(),
        }
    }

    fn with_base_imports(mut self) -> Self {
        self.imports.extend(quote! {
            use core::cell::RefCell;
            use espforge_platform::esp_hal::gpio::{AnyPin, Input, Output, Level, Pin, Pull};
            use espforge_platform::esp_hal::{Blocking, Async};
        });
        self
    }

    fn with_spi_imports_if(mut self, enabled: bool) -> Self {
        if enabled {
            self.imports.extend(quote! {
                use espforge_platform::esp_hal::spi::master::Spi;
                use espforge_platform::bus::SpiDevice;
                use embedded_hal_bus::spi::RefCellDevice as SpiRefCellDevice;
            });
        }
        self
    }

    fn with_i2c_imports_if(mut self, i2c_enabled: bool, spi_enabled: bool) -> Self {
        if i2c_enabled {
            self.imports.extend(quote! {
                use espforge_platform::esp_hal::i2c::master::I2c;
            });

            // Currently, espforge_platform gates the `bus` module behind the `spi` feature.
            // We only import I2cDevice from `bus` if SPI is also enabled.
            if spi_enabled {
                self.imports.extend(quote! {
                    use espforge_platform::bus::I2cDevice;
                    use embedded_hal_bus::i2c::RefCellDevice as I2cRefCellDevice;
                });
            }
        }
        self
    }

    fn build(self) -> TokenStream {
        self.imports
    }
}


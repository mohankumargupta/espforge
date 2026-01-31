use anyhow::{anyhow, Context, Result};
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext, Plugin, PluginKind, PluginRegistration};
use quote::{format_ident, quote};
use inventory;
use proc_macro2::TokenStream;
use serde::Deserialize;
use serde_yaml_ng;
use std::str::FromStr;

#[derive(Deserialize, Debug, Clone)]
pub struct ILI9341Config {
    pub spi: String,
    pub dc: String,
    pub rst: String,
    pub cs: String,
}

pub struct ILI9341Plugin;

impl Plugin for ILI9341Plugin {
    fn name(&self) -> &'static str { "ili9341" }

    fn kind(&self) -> PluginKind { PluginKind::Device }

    fn validate(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: ILI9341Config = serde_yaml_ng::from_value(properties.clone())
            .context("Invalid ILI9341 configuration")?;
        Ok(())
    }

    fn dependencies(&self, properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        let config: ILI9341Config = serde_yaml_ng::from_value(properties.clone())?;
        let spi_name = config.spi.strip_prefix('$').unwrap_or(&config.spi);
        let dc_name = config.dc.strip_prefix('$').unwrap_or(&config.dc);
        let rst_name = config.rst.strip_prefix('$').unwrap_or(&config.rst);
        let cs_name = config.cs.strip_prefix('$').unwrap_or(&config.cs);

        Ok(vec![
            Dependency::component(spi_name),
            Dependency::pin(dc_name),
            Dependency::pin(rst_name),
            Dependency::pin(cs_name),
        ])
    }

    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: ILI9341Config = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Failed to parse ILI9341 properties")?;

        let field_ident = format_ident!("{}", ctx.instance_name);

        // Helper to get raw names
        let spi_name = config.spi.strip_prefix('$').unwrap_or(&config.spi);
        let dc_name = config.dc.strip_prefix('$').unwrap_or(&config.dc);
        let rst_name = config.rst.strip_prefix('$').unwrap_or(&config.rst);
        let cs_name = config.cs.strip_prefix('$').unwrap_or(&config.cs);

        // Retrieve resolved dependencies
        let spi_dep = ctx.resolved_deps.get(spi_name)
            .ok_or_else(|| anyhow!("SPI component '{}' not found", spi_name))?;
        let dc_dep = ctx.resolved_deps.get(dc_name)
            .ok_or_else(|| anyhow!("DC pin '{}' not found", dc_name))?;
        let rst_dep = ctx.resolved_deps.get(rst_name)
            .ok_or_else(|| anyhow!("RST pin '{}' not found", rst_name))?;
        let cs_dep = ctx.resolved_deps.get(cs_name)
            .ok_or_else(|| anyhow!("CS pin '{}' not found", cs_name))?;

        let spi_access: TokenStream = TokenStream::from_str(&spi_dep.access_path)
            .map_err(|e| anyhow!("Failed to parse SPI access path: {}", e))?;
        let dc_access: TokenStream = TokenStream::from_str(&dc_dep.access_path)
            .map_err(|e| anyhow!("Failed to parse DC access path: {}", e))?;
        let rst_access: TokenStream = TokenStream::from_str(&rst_dep.access_path)
            .map_err(|e| anyhow!("Failed to parse RST access path: {}", e))?;
        let cs_access: TokenStream = TokenStream::from_str(&cs_dep.access_path)
            .map_err(|e| anyhow!("Failed to parse CS access path: {}", e))?;

        // Identifiers for intermediate variables
        let cs_pin_ident = format_ident!("{}_cs_pin", ctx.instance_name);
        let dc_pin_ident = format_ident!("{}_dc_pin", ctx.instance_name);
        let rst_pin_ident = format_ident!("{}_rst_pin", ctx.instance_name);
        let spi_device_ident = format_ident!("{}_spi_dev", ctx.instance_name);

        Ok(GeneratedCode {
            field: quote! {
                pub #field_ident: espforge_devices::devices::ili9341::device::ILI9341Device<
                    espforge_platform::bus::SpiDevice<'a>,
                    espforge_platform::gpio::GPIOOutput,
                    espforge_platform::gpio::GPIOOutput
                >
            },
            init: quote! {
                // 1. Acquire Pins from Registry
                let #dc_pin_ident = #dc_access.borrow_mut().take().expect("DC pin already in use");
                let #rst_pin_ident = #rst_access.borrow_mut().take().expect("RST pin already in use");
                let #cs_pin_ident = #cs_access.borrow_mut().take().expect("CS pin already in use");

                // 2. Wrap pins in GPIOOutput
                let #dc_pin_ident = espforge_platform::gpio::GPIOOutput::from_pin(#dc_pin_ident);
                let #rst_pin_ident = espforge_platform::gpio::GPIOOutput::from_pin(#rst_pin_ident);
                let mut #cs_pin_ident = espforge_platform::gpio::GPIOOutput::from_pin(#cs_pin_ident);

                // 3. Ensure CS is High (Inactive) before passing to SpiDevice
                {
                    use embedded_hal::digital::OutputPin;
                    #cs_pin_ident.set_high().ok();
                }

                // 4. Create SPI Device with CS (now accepts GPIOOutput)
                let #spi_device_ident = espforge_platform::bus::SpiDevice::new(
                    #spi_access.bus(),
                    #cs_pin_ident
                );

                // 5. Initialize Display Driver
                let #field_ident = espforge_devices::devices::ili9341::device::ILI9341Device::new(
                    #spi_device_ident,
                    #dc_pin_ident,
                    #rst_pin_ident,
                    delay
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}

inventory::submit! {
    PluginRegistration(&ILI9341Plugin)
}


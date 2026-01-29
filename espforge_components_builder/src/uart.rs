use anyhow::{Context, Result};
use espforge_common::components::uart::UartDeviceConfig;
use espforge_common::dependency::Dependency;
use espforge_common::plugin::{GeneratedCode, GenerationContext, Plugin, PluginKind, PluginRegistration};
use quote::{format_ident, quote};
use serde_yaml_ng;

pub struct UartDevicePlugin;

impl Plugin for UartDevicePlugin {
    fn name(&self) -> &'static str { "UartDevice" }
    fn kind(&self) -> PluginKind { PluginKind::Component }
    
    fn validate(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        let _config: UartDeviceConfig = serde_yaml_ng::from_value(properties.clone())?;
        Ok(())
    }

    fn dependencies(&self, _properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        // We don't return dependency on peripheral here because UARTs are not typically
        // in the generated registry (they are often created on demand via UartDevice).
        // If they were in the registry, we would use Dependency::peripheral.
        Ok(vec![])
    }

    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let config: UartDeviceConfig = serde_yaml_ng::from_value(ctx.properties.clone())
            .context("Invalid UartDevice configuration")?;
        let field_ident = format_ident!("{}", ctx.instance_name);
        
        let uart_name = config.uart.strip_prefix('$').unwrap_or(&config.uart);

        let esp32 = ctx.model.esp32.as_ref().context("ESP32 config missing")?;
        let uart_conf = esp32.uart.get(uart_name).context("UART config missing")?;
        
        let baud = config.baud.unwrap_or(uart_conf.baud);
        
        let uart_num = uart_conf.uart as u8;
        let tx = uart_conf.tx as u8;
        let rx = uart_conf.rx as u8;
        
        Ok(GeneratedCode {
            field: quote! { pub #field_ident: espforge_components::components::uart::Uart },
            init: quote! {
                let #field_ident = espforge_components::components::uart::Uart::new(
                    #uart_num,
                    #tx,
                    #rx,
                    #baud
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}

inventory::submit! {
    PluginRegistration(&UartDevicePlugin)
}

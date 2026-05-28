use anyhow::Result;
use espforge_macros::ComponentPlugin;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(name = "WebSocketClient", features = "websocket,embassy")]
pub struct WebSocketClientPlugin;

impl WebSocketClientPlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        // Validate required properties
        if let Some(uri) = properties.get("uri").and_then(|v| v.as_str()) {
            if !uri.starts_with("ws://") && !uri.starts_with("wss://") {
                return Err(anyhow::anyhow!(
                    "WebSocket URI must start with 'ws://' or 'wss://'"
                ));
            }
        } else {
            return Err(anyhow::anyhow!("WebSocket 'uri' property is required"));
        }
        Ok(())
    }

    fn resolve_dependencies(&self, _properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        // WebSocket requires wifi feature
        Ok(vec![])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let field_ident = format_ident!("{}", ctx.instance_name);

        // Get URI from properties
        let uri = ctx
            .properties
            .get("uri")
            .and_then(|v| v.as_str())
            .unwrap_or("ws://localhost:8080");

        Ok(GeneratedCode {
            field: quote! {
                pub #field_ident: espforge_components::components::websocket::WebSocketClient<'static>,
            },
            init: quote! {
                static WS_RESOURCES: static_cell::StaticCell<
                    espforge_components::components::websocket::WebSocketResources,
                > = static_cell::StaticCell::new();
                let ws_resources = WS_RESOURCES
                    .init(espforge_components::components::websocket::WebSocketResources::new());
                let mut #field_ident = espforge_components::components::websocket::WebSocketClient::new(
                    stack,
                    ws_resources,
                    #uri,
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}


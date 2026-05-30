use anyhow::Result;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use espforge_macros::ComponentPlugin;

#[derive(ComponentPlugin)]
#[plugin(
    name = "WebSocketClient",
    features = "websockets",
    pub_use = "espforge_components::components::websockets::Message"
)]
pub struct WebSocketClientPlugin;

impl WebSocketClientPlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        if let Some(uri) = properties.get("uri").and_then(|v| v.as_str()) {
            if !uri.starts_with("ws://") && !uri.starts_with("wss://") {
                return Err(anyhow::anyhow!(
                    "WebSocket URI must start with ws:// or wss://"
                ));
            }
        } else {
            return Err(anyhow::anyhow!("WebSocket 'uri' property is required"));
        }
        Ok(())
    }

    fn resolve_dependencies(&self, _properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        Ok(vec![])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let instance_name = ctx.instance_name;
        let field_ident = format_ident!("{}", instance_name);

        let uri = ctx
            .properties
            .get("uri")
            .and_then(|v| v.as_str())
            .unwrap_or("ws://localhost:8080");

        let needs_tls = uri.starts_with("wss://");

        let resources_cell = format_ident!("{}_WS_RESOURCES", instance_name.to_uppercase());

        let static_resources: TokenStream = if needs_tls {
            quote! {
                static #resources_cell: static_cell::StaticCell<
                    espforge_components::components::websockets::WebSocketResources
                > = static_cell::StaticCell::new();
            }
        } else {
            quote! {
                static #resources_cell: static_cell::StaticCell<
                    espforge_components::components::websockets::WebSocketResources
                > = static_cell::StaticCell::new();
            }
        };

        let resources_init: TokenStream = if needs_tls {
            quote! {
                #resources_cell.init(
                    espforge_components::components::websockets::WebSocketResources::new_with_tls()
                )
            }
        } else {
            quote! {
                #resources_cell.init(
                    espforge_components::components::websockets::WebSocketResources::new()
                )
            }
        };

        let field: TokenStream = quote! {
            pub #field_ident: espforge_components::components::websockets::WebSocketClient<'static>
        };

        let init: TokenStream = quote! {
            #static_resources
            let #field_ident = {
                let resources = #resources_init;
                espforge_components::components::websockets::WebSocketClient::new(
                    stack,
                    resources,
                    #uri,
                )
            };
        };

        Ok(GeneratedCode {
            field,
            init,
            struct_init: quote! { #field_ident },
        })
    }
}


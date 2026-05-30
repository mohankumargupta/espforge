// espforge_components_builder/src/websockets.rs
//
// Fixes vs first attempt:
//   • Field type now includes the 'static lifetime:
//       WebSocketClient<'static>
//   • Resources stored in a StaticCell (same pattern as http component).
//   • URI literal passed directly to WebSocketClient::new().
//   • No mention of rand_core, esp_hal, or embedded_tls — those are not deps
//     of espforge_components_builder.

use anyhow::Result;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;

#[derive(ComponentPlugin)]
#[plugin(name = "WebSocketClient", features = "websockets")]
pub struct WebSocketClientPlugin;

impl WebSocketClientPlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        if let Some(uri) = properties.get("uri").and_then(|v| v.as_str()) {
            if !uri.starts_with("ws://") && !uri.starts_with("wss://") {
                return Err(anyhow::anyhow!(
                    "WebSocket 'uri' must start with 'ws://' or 'wss://'; got '{}'",
                    uri
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

        // RESOURCES_CELL_IDENT — e.g. WS_CLIENT_WS_RESOURCES
        let resources_cell = format_ident!("{}_WS_RESOURCES", instance_name.to_uppercase());

        let resources_ctor: TokenStream = if needs_tls {
            quote! {
                espforge_components::components::websockets::WebSocketResources::new_with_tls()
            }
        } else {
            quote! {
                espforge_components::components::websockets::WebSocketResources::new()
            }
        };

        // Field in the Components struct
        let field: TokenStream = quote! {
            pub #field_ident:
                espforge_components::components::websockets::WebSocketClient<'static>
        };

        // Initialisation inside Components::new()
        //
        //   static CELL: StaticCell<WebSocketResources> = StaticCell::new();
        //   let resources = CELL.init(WebSocketResources::new());
        //   let ws_client = WebSocketClient::new(stack, resources, uri);
        let init: TokenStream = quote! {
            static #resources_cell: static_cell::StaticCell<
                espforge_components::components::websockets::WebSocketResources
            > = static_cell::StaticCell::new();

            let #field_ident = {
                let resources = #resources_cell.init(#resources_ctor);
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


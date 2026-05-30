// espforge_components_builder/src/websockets.rs
//
// Changes vs original:
//   • WebSocketClient field type now uses 'static lifetime, matching how the
//     static resource cell is set up.
//   • The static resource is stored in a StaticCell so it can be safely
//     initialised once and handed out as `&'static mut`.
//   • URI is passed to WebSocketClient::new at init time rather than stored
//     separately, which matches the runtime API.
//   • TLS variant (`wss://`) allocates `WebSocketResources::new_with_tls()`.

use anyhow::Result;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use espforge_macros::ComponentPlugin;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};

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
        // WebSocketClient only needs the Wi-Fi stack, which is injected via the
        // entry-point template; it has no explicit component dependencies.
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

        // Static name for the resource cell, e.g. `MY_WS_CLIENT_WS_RESOURCES`
        let resources_static =
            format_ident!("{}_WS_RESOURCES", instance_name.to_uppercase());

        // Choose the right resources constructor
        let resources_init: TokenStream = if needs_tls {
            quote! { espforge_components::components::websockets::WebSocketResources::new_with_tls() }
        } else {
            quote! { espforge_components::components::websockets::WebSocketResources::new() }
        };

        // Field declaration in the Components struct
        let field: TokenStream = quote! {
            pub #field_ident: espforge_components::components::websockets::WebSocketClient<'static>
        };

        // Initialisation code emitted inside the Components::new() body.
        //
        // Pattern:
        //   static CELL: StaticCell<WebSocketResources> = StaticCell::new();
        //   let resources: &'static mut _ = CELL.init(WebSocketResources::new());
        //   let ws_client = WebSocketClient::new(stack, resources, uri);
        let init: TokenStream = quote! {
            static #resources_static: static_cell::StaticCell<
                espforge_components::components::websockets::WebSocketResources
            > = static_cell::StaticCell::new();

            let #field_ident = {
                let resources = #resources_static.init(#resources_init);
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


use anyhow::Result;

use espforge_macros::ComponentPlugin;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

// pub_use re-exports Message into the generated crate's root so user app.rs
// can write: use crate::{ Context, component, Message };
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
                    "WebSocket URI must start with 'ws://' or 'wss://'"
                ));
            }
        } else {
            return Err(anyhow::anyhow!("WebSocket 'uri' property is required"));
        }
        Ok(())
    }

    fn resolve_dependencies(
        &self,
        _properties: &serde_yaml_ng::Value,
    ) -> Result<Vec<espforge_configuration::plugin::Dependency>> {
        Ok(vec![])
    }

    fn generate_code(
        &self,
        ctx: &espforge_configuration::plugin::GenerationContext,
    ) -> Result<espforge_configuration::plugin::GeneratedCode> {
        use espforge_configuration::plugin::GeneratedCode;

        let instance_name = ctx.instance_name;
        let field_ident = format_ident!("{}", instance_name);

        let uri = ctx
            .properties
            .get("uri")
            .and_then(|v| v.as_str())
            .unwrap_or("ws://localhost:8080");

        let needs_tls = uri.starts_with("wss://");

        let resources_cell = format_ident!("{}_WS_RESOURCES", instance_name.to_uppercase());

        let static_decl: TokenStream = quote! {
            static #resources_cell: static_cell::StaticCell<
                espforge_components::components::websockets::WebSocketResources
            > = static_cell::StaticCell::new();
        };

        // Field type — WebSocketClient has no lifetime parameter.
        let field: TokenStream = quote! {
            pub #field_ident: espforge_components::components::websockets::WebSocketClient
        };

        let resources_init = if needs_tls {
            quote! {
                espforge_components::components::websockets::WebSocketResources::new_with_tls()
            }
        } else {
            quote! {
                espforge_components::components::websockets::WebSocketResources::new()
            }
        };

        // Correct arg order: (stack, uri, resources)
        let init: TokenStream = quote! {
            #static_decl
            let resources = #resources_cell.init(#resources_init);
            let #field_ident =
                espforge_components::components::websockets::WebSocketClient::new(
                    stack,
                    #uri,
                    resources,
                );
        };

        Ok(GeneratedCode {
            field,
            init,
            struct_init: quote! { #field_ident },
        })
    }
}

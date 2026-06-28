use anyhow::Result;
use espforge_macros::ComponentPlugin;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(
    name = "WebSocketClient", 
    features = "websockets", 
    pub_use = "espforge_components::Message" 
)]
pub struct WebSocketClientPlugin;

impl WebSocketClientPlugin {
    fn validate_properties(&self, properties: &serde_yaml_ng::Value) -> Result<()> {
        if let Some(uri) = properties.get("uri").and_then(|v| v.as_str()) {
            if !uri.starts_with("ws://") && !uri.starts_with("wss://") {
                return Err(anyhow::anyhow!(
                    "WebSocket 'uri' must start with ws:// or wss://"
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

        let field: TokenStream = quote! {
            pub #field_ident: espforge_components::components::websockets::WebSocketClient<'static>
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

        let tls_init: TokenStream = if needs_tls {
            quote! {
                {
                    static RNG_CELL: static_cell::StaticCell<espforge_platform::esp_hal::rng::Rng> = static_cell::StaticCell::new();
                    static TLS_CELL: static_cell::StaticCell<mbedtls_rs::Tls<'static>> = static_cell::StaticCell::new();
                    let rng = RNG_CELL.init(espforge_platform::esp_hal::rng::Rng::new());
                    let tls = TLS_CELL.init(mbedtls_rs::Tls::new(rng).expect("Failed to initialize mbedtls"));
                    Some(tls.reference())
                }
            }
        } else {
            quote! { None }
        };

        let init: TokenStream = quote! {
            #static_decl
            let #field_ident = espforge_components::components::websockets::WebSocketClient::new(
                stack,
                #uri,
                #resources_init,
                #tls_init
            );
        };

        Ok(GeneratedCode {
            field,
            init,
            struct_init: quote! { #field_ident },
        })
    }
}


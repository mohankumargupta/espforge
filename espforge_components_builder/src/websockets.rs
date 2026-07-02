use anyhow::Result;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(
    name = "WebSocketClient",
    features = "websockets,embassy",
    pub_use = "espforge_components::Message"
)]
pub struct WebSocketClientPlugin;

impl WebSocketClientPlugin {
    fn validate_properties(&self, _properties: &serde_yaml_ng::Value) -> Result<()> {
        // No required properties — the connector is endpoint-agnostic.
        // The URI is passed to `connect()` at runtime from app code, not
        // baked in at codegen time.
        Ok(())
    }

    fn resolve_dependencies(&self, _properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        // Relies on `stack`, which the entry-point template injects whenever
        // any component needs networking (see `needs_stack` in
        // espforge_codegen/src/templates/entry_point.rs).
        Ok(vec![])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let field_ident = format_ident!("{}", ctx.instance_name);
        let resources_static = format_ident!("{}_WS_RESOURCES", ctx.instance_name.to_uppercase());

        Ok(GeneratedCode {
            // The high-level client owns the connector plus the TLS/RNG state
            // so app code can just call `.connect(uri)` without juggling
            // SessionContext or mbedtls setup.
            field: quote! {
                pub #field_ident: espforge_components::components::websockets::WebSocketClient
            },
            init: quote! {
                static #resources_static: static_cell::StaticCell<espforge_components::components::websockets::WebSocketResources> = static_cell::StaticCell::new();

                let connector = espforge_components::components::websockets::WebSocketConnector::new(
                    stack,
                    #resources_static.init(
                        espforge_components::components::websockets::WebSocketResources::new()
                    )
                );

                let #field_ident = espforge_components::components::websockets::WebSocketClient::new(connector);
            },
            struct_init: quote! { #field_ident },
        })
    }
}


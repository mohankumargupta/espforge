use anyhow::Result;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(name = "WebSocketClient", features = "websockets")]
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

    fn pub_use(&self) -> Vec<&'static str> {
        vec!["espforge_components::components::websockets::Message"]
    }

fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
    let field_ident = format_ident!("{}", ctx.instance_name);

    let uri = ctx
        .properties
        .get("uri")
        .and_then(|v| v.as_str())
        .unwrap_or("ws://localhost:8080");

    let uri_lit = syn::LitStr::new(uri, proc_macro2::Span::call_site());

    let resources_ident = format_ident!("{}_WS_RESOURCES", ctx.instance_name.to_uppercase());

    Ok(GeneratedCode {
        field: quote! {
            pub #field_ident: espforge_components::components::websockets::WebSocketClient<'static>
        },
        init: quote! {
            static #resources_ident: static_cell::StaticCell<espforge_components::components::websockets::WebSocketResources> = static_cell::StaticCell::new();
            let #field_ident = espforge_components::components::websockets::WebSocketClient::new(
                stack,
                #resources_ident.init(espforge_components::components::websockets::WebSocketResources::new()),
                #uri_lit,
            );
        },
        struct_init: quote! { #field_ident },
    })
}
}

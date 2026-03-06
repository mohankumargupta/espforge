use anyhow::Result;
use espforge_macros::ComponentPlugin;
//use proc_macro2::TokenStream;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(name = "HttpClient", features = "http,embassy")]
pub struct HttpClientPlugin;

impl HttpClientPlugin {
    fn validate_properties(&self, _properties: &serde_yaml_ng::Value) -> Result<()> {
        Ok(())
    }

    fn resolve_dependencies(&self, _properties: &serde_yaml_ng::Value) -> Result<Vec<Dependency>> {
        Ok(vec![])
    }

    fn generate_code(&self, ctx: &GenerationContext) -> Result<GeneratedCode> {
        let field_ident = format_ident!("{}", ctx.instance_name);
        Ok(GeneratedCode {
            field: quote! {
                pub #field_ident: espforge_components::components::http::HttpClient,
            },
            init: quote! {
                static HTTP_RESOURCES: static_cell::StaticCell<
                    espforge_components::components::http::HttpResources,
                > = static_cell::StaticCell::new();
                let http_resources = HTTP_RESOURCES
                    .init(espforge_components::components::http::HttpResources::new());
                let #field_ident = espforge_components::components::http::HttpClient::new(
                    stack,
                    http_resources,
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}

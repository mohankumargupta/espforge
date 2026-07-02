use anyhow::Result;
use espforge_configuration::plugin::{Dependency, GeneratedCode, GenerationContext};
use espforge_macros::ComponentPlugin;
use quote::{format_ident, quote};

#[derive(ComponentPlugin)]
#[plugin(name = "HttpsClient", features = "https,embassy")]
pub struct HttpsClientPlugin;

impl HttpsClientPlugin {
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
                pub #field_ident: espforge_components::components::https::HttpsClient,
            },
            init: quote! {
                static HTTPS_RESOURCES: static_cell::StaticCell<
                    espforge_components::components::https::HttpsResources,
                > = static_cell::StaticCell::new();

                let https_resources = HTTPS_RESOURCES
                    .init(espforge_components::components::https::HttpsResources::new());

                let #field_ident = espforge_components::components::https::HttpsClient::new(
                    stack,
                    https_resources,
                );
            },
            struct_init: quote! { #field_ident },
        })
    }
}

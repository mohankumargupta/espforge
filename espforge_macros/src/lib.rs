use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr, parse_macro_input};

#[proc_macro_derive(ComponentPlugin, attributes(plugin))]
pub fn derive_component_plugin(input: TokenStream) -> TokenStream {
    derive_plugin_internal(
        input,
        quote!(::espforge_configuration::plugin::PluginKind::Component),
    )
}

#[proc_macro_derive(DevicePlugin, attributes(plugin))]
pub fn derive_device_plugin(input: TokenStream) -> TokenStream {
    derive_plugin_internal(
        input,
        quote!(::espforge_configuration::plugin::PluginKind::Device),
    )
}

fn derive_plugin_internal(input: TokenStream, kind_path: proc_macro2::TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let mut required_features = quote!(vec![]);

    // Extract name from #[plugin(name = "...")]
    let plugin_name = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("plugin"))
        .find_map(|a| {
            let mut name_val = None;
            let _ = a.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value = meta.value()?;
                    let lit: LitStr = value.parse()?;
                    name_val = Some(lit.value());
                }

                if meta.path.is_ident("features") {
                    let value = meta.value()?;
                    let lit: LitStr = value.parse()?;
                    let feats: Vec<String> = lit
                        .value()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    required_features = quote!(vec![#(#feats.to_string()),*]);
                }

                Ok(())
            });
            name_val
        })
        .unwrap_or_else(|| name.to_string().trim_end_matches("Plugin").to_string());

    let expanded = quote! {
        impl ::espforge_configuration::plugin::Plugin for #name {
            fn name(&self) -> &'static str {
                #plugin_name
            }

            fn kind(&self) -> ::espforge_configuration::plugin::PluginKind {
                #kind_path
            }

            fn validate(&self, properties: &::serde_yaml_ng::Value) -> ::anyhow::Result<()> {
                self.validate_properties(properties)
            }

            fn dependencies(&self, properties: &::serde_yaml_ng::Value)
                -> ::anyhow::Result<::std::vec::Vec<::espforge_configuration::plugin::Dependency>> {
                self.resolve_dependencies(properties)
            }

            fn generate(&self, ctx: &::espforge_configuration::plugin::GenerationContext)
                -> ::anyhow::Result<::espforge_configuration::plugin::GeneratedCode> {
                self.generate_code(ctx)
            }

            fn required_features(&self) -> Vec<String> { #required_features }
        }

        ::inventory::submit! {
            ::espforge_configuration::plugin::PluginRegistration(&#name)
        }
    };

    TokenStream::from(expanded)
}

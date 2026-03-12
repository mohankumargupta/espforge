use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, LitStr};

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
    impl_plugin(&input, kind_path)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn impl_plugin(
    st: &DeriveInput,
    kind_path: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let name = &st.ident;

    let mut plugin_name_val: Option<String> = None;
    let mut required_features = quote!(vec![]);
    let mut config_type: Option<syn::Type> = None;

    for attr in &st.attrs {
        if attr.path().is_ident("plugin") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value = meta.value()?;
                    let lit: LitStr = value.parse()?;
                    plugin_name_val = Some(lit.value());
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
                if meta.path.is_ident("config") {
                    let value = meta.value()?;
                    let lit: LitStr = value.parse()?;
                    let ty_str = lit.value();
                    let ty: syn::Type = syn::parse_str(&ty_str)
                        .map_err(|e| meta.error(e.to_string()))?;
                    config_type = Some(ty);
                }
                Ok(())
            });
        }
    }

    let plugin_name = plugin_name_val.unwrap_or_else(|| {
        name.to_string()
            .trim_end_matches("Plugin")
            .to_string()
    });

    // Generate the three trait method bodies depending on whether a typed config is used.
    let (validate_body, dependencies_body, generate_body) = if let Some(ref cfg_ty) = config_type {
        (
            quote! {
                let config: #cfg_ty = serde_yaml_ng::from_value(properties.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize config for '{}': {}", #plugin_name, e))?;
                self.validate_config(&config)
            },
            quote! {
                let config: #cfg_ty = serde_yaml_ng::from_value(properties.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize config for '{}': {}", #plugin_name, e))?;
                self.resolve_dependencies(&config)
            },
            quote! {
                let config: #cfg_ty = serde_yaml_ng::from_value(ctx.properties.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize config for '{}': {}", #plugin_name, e))?;
                self.generate_code(&config, ctx)
            },
        )
    } else {
        (
            quote! { self.validate_properties(properties) },
            quote! { self.resolve_dependencies(properties) },
            quote! { self.generate_code(ctx) },
        )
    };

    let expanded = quote! {
        impl ::espforge_configuration::plugin::Plugin for #name {
            fn name(&self) -> &'static str {
                #plugin_name
            }

            fn kind(&self) -> ::espforge_configuration::plugin::PluginKind {
                #kind_path
            }

            fn required_features(&self) -> Vec<String> {
                #required_features
            }

            fn validate(
                &self,
                properties: &serde_yaml_ng::Value,
            ) -> anyhow::Result<()> {
                #validate_body
            }

            fn dependencies(
                &self,
                properties: &serde_yaml_ng::Value,
            ) -> anyhow::Result<Vec<::espforge_configuration::plugin::Dependency>> {
                #dependencies_body
            }

            fn generate(
                &self,
                ctx: &::espforge_configuration::plugin::GenerationContext,
            ) -> anyhow::Result<::espforge_configuration::plugin::GeneratedCode> {
                #generate_body
            }
        }

        inventory::submit! {
            ::espforge_configuration::plugin::PluginRegistration(&#name)
        }
    };

    Ok(expanded)
}

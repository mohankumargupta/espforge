use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    DeriveInput, Error, Field, GenericArgument, LitStr, PathArguments, Type, parse_macro_input,
};

#[proc_macro_derive(Asker, attributes(asker, input, confirm, secret, select, multiselect))]
pub fn asker_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    impl_asker(&ast)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_derive(EnumAsker, attributes(asker))]
pub fn enum_asker_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    impl_enum_asker(&ast)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn impl_asker(st: &DeriveInput) -> syn::Result<TokenStream2> {
    let struct_name = &st.ident;
    let asker_name = syn::Ident::new(&format!("{}Asker", struct_name), struct_name.span());

    let fields = match &st.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(fields),
            ..
        }) => &fields.named,
        _ => return Err(Error::new_spanned(st, "Asker only supports named structs")),
    };

    let mut builder_fields = quote! {};
    let mut builder_init = quote! {};
    let mut build_logic = quote! {};
    let mut methods = quote! {};

    for field in fields {
        let name = field.ident.as_ref().unwrap();
        let ty = &field.ty;

        let inner_ty = extract_inner_type(ty);
        let is_optional = inner_ty.is_some();
        let target_ty = inner_ty.unwrap_or(ty);

        // Builder state
        builder_fields.extend(quote! { #name: Option<#ty>, });
        builder_init.extend(quote! { #name: None, });

        // Final build logic
        if is_optional {
            build_logic.extend(quote! { #name: self.#name.clone().flatten(), });
        } else {
            build_logic.extend(quote! {
                #name: self.#name.clone().expect(concat!("Field ", stringify!(#name), " not set")),
            });
        }

        let conversion = if is_optional {
            quote! { Some(val) }
        } else {
            quote! { val }
        };

        if has_attr(field, "confirm") {
            let prompt_attr = get_attr_prompt(field, "confirm");

            if let Some(p) = prompt_attr {
                methods.extend(quote! {
                    pub fn #name(mut self) -> Self {
                        let val = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                            .with_prompt(#p)
                            .interact()
                            .unwrap();
                        self.#name = Some(#conversion);
                        self
                    }
                });
            } else {
                methods.extend(quote! {
                    pub fn #name(mut self, prompt: impl Into<String>) -> Self {
                        let val = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                            .with_prompt(prompt.into())
                            .interact()
                            .unwrap();
                        self.#name = Some(#conversion);
                        self
                    }
                });
            }
        } else {
            // Input (default)
            let prompt_attr = get_attr_prompt(field, "input");

            if let Some(p) = prompt_attr {
                methods.extend(quote! {
                    pub fn #name(mut self) -> Self {
                        let val: #target_ty = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                            .with_prompt(#p)
                            .interact_text()
                            .unwrap();
                        self.#name = Some(#conversion);
                        self
                    }
                });
            } else {
                methods.extend(quote! {
                    pub fn #name(mut self, prompt: impl Into<String>) -> Self {
                        let val: #target_ty = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                            .with_prompt(prompt.into())
                            .interact_text()
                            .unwrap();
                        self.#name = Some(#conversion);
                        self
                    }
                });
            }
        }
    }

    Ok(quote! {
        pub struct #asker_name {
            #builder_fields
        }

        impl #asker_name {
            pub fn new() -> Self {
                Self { #builder_init }
            }

            pub fn finish(&self) -> #struct_name {
                #struct_name { #build_logic }
            }

            #methods
        }

        impl #struct_name {
            pub fn asker() -> #asker_name {
                #asker_name::new()
            }
        }
    })
}

fn impl_enum_asker(st: &DeriveInput) -> syn::Result<TokenStream2> {
    let enum_name = &st.ident;
    let variants = match &st.data {
        syn::Data::Enum(d) => &d.variants,
        _ => return Err(Error::new_spanned(st, "EnumAsker only supports enums")),
    };

    let mut prompt = "Select an option".to_string();
    if let Some(attr) = st.attrs.iter().find(|a| a.path().is_ident("asker")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("prompt") {
                let val: LitStr = meta.value()?.parse()?;
                prompt = val.value();
            }
            Ok(())
        });
    }

    let mut labels = Vec::new();
    let mut groups = Vec::new();
    let mut match_arms = Vec::new();
    let mut from_str_arms = Vec::new();

    for (idx, variant) in variants.iter().enumerate() {
        let ident = &variant.ident;
        let mut label = ident.to_string();
        let mut group = String::new();

        if let Some(attr) = variant.attrs.iter().find(|a| a.path().is_ident("asker")) {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("label") {
                    let val: LitStr = meta.value()?.parse()?;
                    label = val.value();
                }
                if meta.path.is_ident("group") {
                    let val: LitStr = meta.value()?.parse()?;
                    group = val.value();
                }
                Ok(())
            });
        }

        labels.push(label.clone());
        groups.push(group);
        match_arms.push(quote! { #idx => #enum_name::#ident, });
        from_str_arms.push(quote! { #label => #enum_name::#ident, });
    }

    Ok(quote! {
        impl #enum_name {
            pub fn ask() -> Self {
                let items = vec![#(#labels),*];
                let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt(#prompt)
                    .default(0)
                    .items(&items)
                    .interact()
                    .unwrap();

                match selection {
                    #(#match_arms)*
                    _ => unreachable!(),
                }
            }

            pub fn ask_filtered(group_filter: &str) -> Self {
                let all_labels = vec![#(#labels),*];
                let all_groups = vec![#(#groups),*];

                // Filter items where group matches or group is empty (common items)
                let filtered_items: Vec<&str> = all_labels.iter()
                    .zip(all_groups.iter())
                    .filter(|(_, g)| {
                        // Strict type assertion to avoid ambiguity
                        let g_str: &str = *g;
                        g_str.is_empty() || g_str == group_filter
                    })
                    .map(|(l, _)| *l)
                    .collect();

                if filtered_items.is_empty() {
                    panic!("No options available for group: {}", group_filter);
                }

                let selection_idx = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt(format!("{} ({})", #prompt, group_filter))
                    .default(0)
                    .items(&filtered_items)
                    .interact()
                    .unwrap();

                let selected_label = filtered_items[selection_idx];

                // Map back to enum variant
                match selected_label {
                    #(#from_str_arms)*
                    _ => unreachable!(),
                }
            }
        }
    })
}

fn extract_inner_type(ty: &Type) -> Option<&Type> {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            if seg.ident == "Option" {
                if let PathArguments::AngleBracketed(args) = &seg.arguments {
                    if let Some(GenericArgument::Type(inner)) = args.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

fn has_attr(field: &Field, name: &str) -> bool {
    field.attrs.iter().any(|a| a.path().is_ident(name))
}

fn get_attr_prompt(field: &Field, attr_name: &str) -> Option<String> {
    for attr in &field.attrs {
        if attr.path().is_ident(attr_name) {
            let mut prompt = None;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("prompt") {
                    if let Ok(val) = meta.value() {
                        if let Ok(lit) = val.parse::<LitStr>() {
                            prompt = Some(lit.value());
                        }
                    }
                }
                Ok(())
            });
            if prompt.is_some() {
                return prompt;
            }
        }
    }
    None
}

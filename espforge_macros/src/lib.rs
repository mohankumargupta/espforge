use proc_macro::TokenStream;
use quote::quote;
use syn::{Expr, ItemStruct, Token, parse_macro_input, punctuated::Punctuated};

/// Attribute macro to automatically call `register_nibbler!` for a struct.
#[proc_macro_attribute]
pub fn auto_register_nibbler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;

    let expanded = quote! {
        #input
        // The expanded macro calls the register_nibbler! macro, which must be
        // in scope (via inventory).
        register_nibbler!(#name);
    };

    expanded.into()
}

/// Attribute macro to automatically call `register_action_strategy!` for a struct.
#[proc_macro_attribute]
pub fn auto_register_action_strategy(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;

    let expanded = quote! {
        #input
        register_action_strategy!(#name);
    };

    expanded.into()
}

/// Attribute macro to automatically call `register_strategy!` for a struct.
/// Takes one or more comma-separated ParameterType expressions as arguments.
/// E.g., `#[auto_register_param_strategy(ParameterType::String, ParameterType::Integer)]`
#[proc_macro_attribute]
pub fn auto_register_param_strategy(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parameter_types =
        parse_macro_input!(attr with Punctuated::<Expr, Token![,]>::parse_terminated);
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let name_ty = name;

    let mut registrations = quote! {};

    for param_type in parameter_types {
        registrations.extend(quote! {
            register_strategy!(#param_type, #name_ty);
        });
    }

    let expanded = quote! {
        #input
        #registrations
    };

    expanded.into()
}

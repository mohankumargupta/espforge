use anyhow::{Context, Result};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use espforge_configuration::EspforgeConfiguration;
use crate::registry::find_plugin;

use crate::templates::{common::format_generated_source, constants::origins};

pub fn generate_lib_source(additional_modules: &[String],  model: &EspforgeConfiguration) -> Result<String> {
    let extra_uses: Vec<&'static str> = model.components.values()
        .filter_map(|spec| find_plugin(&spec.driver))
        .flat_map(|p| p.pub_use())
        .collect();

    let builder = LibraryBuilder::new(additional_modules, &extra_uses);
    let tokens = builder.build();
    let syntax_tree = syn::parse2(tokens).context("Failed to parse generated library structure")?;
    let content = prettyplease::unparse(&syntax_tree);

    Ok(format_generated_source(&content, origins::LIB))
}

struct LibraryBuilder<'a> {
    additional_modules: &'a [String],
    extra_uses: &'a [&'static str], 
}

impl<'a> LibraryBuilder<'a> {
    fn new(additional_modules: &'a [String], extra_uses: &'a [&'static str]) -> Self {
        Self { additional_modules, extra_uses }
    }

    fn build(self) -> TokenStream {
        let mod_declarations = self.module_declarations();
        let macros = self.access_macros();
        let pub_uses = self.pub_uses(); 

        quote! {
            #![no_std]

            #mod_declarations

            pub mod generated;

            pub use generated::{Components, Devices, PeripheralRegistry};
            pub use espforge_platform::Context;

            pub static mut REGISTRY: *mut PeripheralRegistry = core::ptr::null_mut();
            pub static mut COMPONENTS: *mut Components = core::ptr::null_mut();
            pub static mut DEVICES: *mut Devices = core::ptr::null_mut();

            #macros
            #pub_uses
        }
    }

    fn module_declarations(&self) -> TokenStream {
        let declarations = self.additional_modules.iter().map(|module| {
            let mod_ident = format_ident!("{}", module);
            quote! { pub mod #mod_ident; }
        });

        quote! { #(#declarations)* }
    }

    fn access_macros(&self) -> TokenStream {
        quote! {
            /// Access a component from the global static registry
            #[macro_export]
            macro_rules! component {
                ($name:ident) => {
                    // Dereference the static COMPONENTS pointer to access the struct
                    unsafe { &mut (*$crate::COMPONENTS).$name }
                };
            }

            /// Access a device from the global static registry
            #[macro_export]
            macro_rules! device {
                ($name:ident) => {
                    // Dereference the static DEVICES pointer to access the struct
                    unsafe { &mut (*$crate::DEVICES).$name }
                };
            }

            /// Execute a closure with mutable access to a component
            #[macro_export]
            macro_rules! with_component {
                ($name:ident, |$var:ident| $body:block) => {{
                    let $var = unsafe { &mut (*$crate::COMPONENTS).$name };
                    $body
                }};
            }

            /// Execute a closure with mutable access to a device
            #[macro_export]
            macro_rules! with_device {
                ($name:ident, |$var:ident| $body:block) => {{
                    let $var = unsafe { &mut (*$crate::DEVICES).$name };
                    $body
                }};
            }
        }
    }

    fn pub_uses(&self) -> TokenStream {
        self.extra_uses.iter().map(|path| {
            let ts: proc_macro2::TokenStream = path.parse().unwrap();
            quote! { pub use #ts; }
        }).collect()
    }
}

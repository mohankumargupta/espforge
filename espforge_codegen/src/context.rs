use crate::registry::find_plugin;
use crate::resolver::DependencyResolver;
use anyhow::{Context, Result};
use espforge_configuration::{
    EspforgeConfiguration,
    plugin::{GenerationContext, PluginKind, ResolvedDependency},
};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;

pub struct CodegenContext {
    pub model: EspforgeConfiguration,
    pub resolved_deps: HashMap<String, ResolvedDependency>,
    pub init_order: Vec<String>,
}

impl CodegenContext {
    pub fn build(model: &EspforgeConfiguration) -> Result<Self> {
        let resolver = DependencyResolver::new(model)?;
        let init_order = resolver.resolve()?;

        Ok(Self {
            model: model.clone(),
            resolved_deps: HashMap::new(),
            init_order,
        })
    }

    pub fn generate(&self) -> Result<TokenStream> {
        let registry = self.generate_peripheral_registry()?;
        let components = self.generate_components()?;
        let devices = self.generate_devices()?;

        Ok(quote! {
            #registry

            #components

            #devices

            pub struct Context<'a> {
                pub logger: espforge_platform::logger::Logger,
                pub delay: espforge_platform::delay::Delay,
                pub registry: &'a PeripheralRegistry,
                pub components: Components<'a>,
                pub devices: Devices<'a>,
            }
        })
    }

    fn generate_peripheral_registry(&self) -> Result<TokenStream> {
        crate::generator::hardware::generate_peripheral_registry(&self.model)
    }

    fn generate_components(&self) -> Result<TokenStream> {
        let mut fields = vec![quote! { _marker: core::marker::PhantomData<&'a ()> }];
        let mut inits = vec![];
        let mut struct_inits = vec![quote! { _marker: core::marker::PhantomData }];

        // We iterate based on the topological sort order to ensure dependencies are initialized first
        for name in &self.init_order {
            if let Some(spec) = self.model.components.get(name) {
                let plugin = find_plugin(&spec.driver)
                    .ok_or_else(|| anyhow::anyhow!("Unknown component driver: {}", spec.driver))?;

                if plugin.kind() == PluginKind::Component {
                    let deps = self.resolve_deps_for(name)?;

                    let ctx = GenerationContext {
                        model: &self.model,
                        instance_name: name,
                        properties: &spec.properties,
                        resolved_deps: &deps,
                    };

                    let code = plugin
                        .generate(&ctx)
                        .with_context(|| format!("Failed to generate component: {}", name))?;

                    fields.push(code.field);
                    inits.push(code.init);
                    struct_inits.push(code.struct_init);
                }
            }
        }

        Ok(quote! {
            pub struct Components<'a> {
                #(#fields),*
            }

            impl<'a> Components<'a> {
                pub fn new(registry: &'a PeripheralRegistry) -> Self {
                    #(#inits)*

                    Self {
                        #(#struct_inits),*
                    }
                }
            }
        })
    }

    fn generate_devices(&self) -> Result<TokenStream> {
        let mut fields = vec![quote! { _marker: core::marker::PhantomData<&'a ()> }];
        let mut inits = vec![];
        let mut struct_inits = vec![quote! { _marker: core::marker::PhantomData }];

        for name in &self.init_order {
            if let Some(spec) = self.model.devices.get(name) {
                let plugin = find_plugin(&spec.driver)
                    .ok_or_else(|| anyhow::anyhow!("Unknown device driver: {}", spec.driver))?;

                if plugin.kind() == PluginKind::Device {
                    let deps = self.resolve_deps_for(name)?;

                    let ctx = espforge_configuration::plugin::GenerationContext {
                        model: &self.model,
                        instance_name: name,
                        properties: &spec.properties,
                        resolved_deps: &deps,
                    };

                    let code = plugin
                        .generate(&ctx)
                        .with_context(|| format!("Failed to generate device: {}", name))?;

                    fields.push(code.field);
                    inits.push(code.init);
                    struct_inits.push(code.struct_init);
                }
            }
        }

        Ok(quote! {
            pub struct Devices<'a> {
                #(#fields),*
            }

            impl<'a> Devices<'a> {
                pub fn new(
                    registry: &'a PeripheralRegistry,
                    components: &mut Components<'a>,
                    delay: &mut espforge_platform::delay::Delay
                ) -> Self {
                    #(#inits)*

                    Self {
                        #(#struct_inits),*
                    }
                }
            }
        })
    }

    fn resolve_deps_for(&self, instance: &str) -> Result<HashMap<String, ResolvedDependency>> {
        let mut resolved = HashMap::new();

        let (driver, props) = if let Some(c) = self.model.components.get(instance) {
            (&c.driver, &c.properties)
        } else if let Some(d) = self.model.devices.get(instance) {
            (&d.driver, &d.properties)
        } else {
            return Ok(resolved);
        };

        if let Some(plugin) = find_plugin(driver) {
            if let Ok(deps) = plugin.dependencies(props) {
                for dep in deps {
                    let dep_name = dep.name.strip_prefix('$').unwrap_or(&dep.name);

                    let access_path = if self.model.components.contains_key(dep_name) {
                        format!("components.{}", dep_name)
                    } else if self.model.devices.contains_key(dep_name) {
                        format!("devices.{}", dep_name)
                    } else {
                        // Assume it is a hardware resource
                        format!("registry.{}", dep_name)
                    };

                    resolved.insert(
                        dep_name.to_string(),
                        ResolvedDependency {
                            name: dep_name.to_string(),
                            access_path,
                            kind: dep.kind,
                        },
                    );
                }
            }
        }

        Ok(resolved)
    }
}

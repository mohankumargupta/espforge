use anyhow::Result;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use crate::dependency::{Dependency, ResolvedDependency};
use crate::EspforgeConfiguration;
use proc_macro2::TokenStream;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PluginKind {
    Component,
    Device,
}

pub struct GeneratedCode {
    pub field: TokenStream,
    pub init: TokenStream,
    pub struct_init: TokenStream,
}

pub struct GenerationContext<'a> {
    pub instance_name: &'a str,
    pub properties: &'a Value,
    pub resolved_deps: &'a HashMap<String, ResolvedDependency>,
    pub model: &'a EspforgeConfiguration,
}

pub trait Plugin: Sync + Send {
    fn name(&self) -> &'static str;
    fn kind(&self) -> PluginKind;
    fn validate(&self, _properties: &Value) -> Result<()> {
        Ok(())
    }
    fn dependencies(&self, _properties: &Value) -> Result<Vec<Dependency>> {
        Ok(vec![])
    }
    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode>;
}

pub struct PluginRegistration(pub &'static dyn Plugin);

inventory::collect!(PluginRegistration);
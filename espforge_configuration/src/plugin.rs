use anyhow::Result;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use crate::EspforgeConfiguration;
use proc_macro2::TokenStream;

// Note: dependency module was not moved, but Plugin relies on it. 
// We need to decide where Dependency lives. 
// If Dependency is used for graph resolution, it belongs in codegen or configuration.
// Let's assume it's part of the Plugin trait interface.
// For now, we will define a simple struct here or import if it exists.
// Looking at the original file list, `espforge_codegen` had `dependency.rs`.
// It seems `espforge_common` used to import it from `crate::dependency` but that file was missing in common's file list?
// Ah, `espforge_codegen/src/dependency.rs` exists. `espforge_common` did NOT have `dependency.rs` in the dump.
// The error log `unresolved import crate::dependency` confirms it was missing.
// We must add the Dependency struct here.

pub struct Dependency {
    pub name: String,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyKind {
    Component,
    Device,
    Peripheral,
    Pin,
}

impl Dependency {
    pub fn component(name: impl Into<String>) -> Self {
        Self { name: name.into(), kind: DependencyKind::Component }
    }
    pub fn device(name: impl Into<String>) -> Self {
        Self { name: name.into(), kind: DependencyKind::Device }
    }
    pub fn peripheral(name: impl Into<String>) -> Self {
        Self { name: name.into(), kind: DependencyKind::Peripheral }
    }
    pub fn pin(name: impl Into<String>) -> Self {
        Self { name: name.into(), kind: DependencyKind::Pin }
    }
}

pub struct ResolvedDependency {
    pub kind: DependencyKind,
    pub name: String,
    pub access_path: String,
}

#[derive(PartialEq)]
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
    pub model: &'a EspforgeConfiguration,
    pub instance_name: &'a str,
    pub properties: &'a Value,
    pub resolved_deps: &'a HashMap<String, ResolvedDependency>,
}

pub trait Plugin: Sync + Send {
    fn name(&self) -> &'static str;
    fn kind(&self) -> PluginKind;
    fn validate(&self, _properties: &Value) -> Result<()> { Ok(()) }
    fn dependencies(&self, _properties: &Value) -> Result<Vec<Dependency>> { Ok(vec![]) }
    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode>;
}

pub struct PluginRegistration(pub &'static dyn Plugin);

inventory::collect!(PluginRegistration);


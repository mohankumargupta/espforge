use crate::EspforgeConfiguration;
use anyhow::{anyhow, Result};
use proc_macro2::TokenStream;
use serde_yaml_ng::Value;
use std::collections::HashMap;
pub use crate::refs::{ComponentRef, DeviceRef, PinRef};

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
        Self {
            name: name.into(),
            kind: DependencyKind::Component,
        }
    }
    pub fn device(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: DependencyKind::Device,
        }
    }
    pub fn peripheral(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: DependencyKind::Peripheral,
        }
    }
    pub fn pin(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: DependencyKind::Pin,
        }
    }

    pub fn component_ref(r: &DeviceRef<ComponentRef>) -> Self {
        Self::component(r.as_str())
    }

    pub fn pin_ref(r: &DeviceRef<PinRef>) -> Self {
        Self::pin(r.as_str())
    }
}

impl std::fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyKind::Component => write!(f, "Component"),
            DependencyKind::Device => write!(f, "Device"),
            DependencyKind::Peripheral => write!(f, "Peripheral"),
            DependencyKind::Pin => write!(f, "Pin"),
        }
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

impl<'a> GenerationContext<'a> {
    /// Normalizes references that may be written as `$name` in YAML.
    pub fn normalize_ref_name<'s>(&self, name: &'s str) -> &'s str {
        name.strip_prefix('$').unwrap_or(name)
    }

    /// Returns a resolved dependency by name and validates its expected kind.
    pub fn dependency(
        &self,
        name: &str,
        expected: DependencyKind,
    ) -> Result<&ResolvedDependency> {
        let normalized = self.normalize_ref_name(name);
        let dep = self.resolved_deps.get(normalized).ok_or_else(|| {
            anyhow!(
                "Dependency '{}' not found for instance '{}'",
                normalized,
                self.instance_name
            )
        })?;

        if dep.kind != expected {
            return Err(anyhow!(
                "Dependency '{}' has kind '{}', expected '{}' for instance '{}'",
                normalized,
                dep.kind,
                expected,
                self.instance_name
            ));
        }

        Ok(dep)
    }

    /// Resolves and parses a dependency access path as a TokenStream.
    pub fn dependency_access(&self, name: &str, expected: DependencyKind) -> Result<TokenStream> {
        let dep = self.dependency(name, expected)?;
        dep.access_path.parse::<TokenStream>().map_err(|e| {
            anyhow!(
                "Failed to parse access path '{}' for dependency '{}': {}",
                dep.access_path, dep.name, e
            )
        })
    }
}




pub trait Plugin: Sync + Send {
    fn name(&self) -> &'static str;
    fn kind(&self) -> PluginKind;
    fn validate(&self, _properties: &Value) -> Result<()> {
        Ok(())
    }
    fn required_features(&self) -> Vec<String> {
        vec![]
    }
    fn dependencies(&self, _properties: &Value) -> Result<Vec<Dependency>> {
        Ok(vec![])
    }
    fn generate(&self, ctx: &GenerationContext) -> Result<GeneratedCode>;
}

pub struct PluginRegistration(pub &'static dyn Plugin);

inventory::collect!(PluginRegistration);

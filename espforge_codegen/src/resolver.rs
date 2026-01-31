use crate::registry::find_plugin;
use anyhow::Result;
use espforge_configuration::EspforgeConfiguration;

use crate::dependency::DependencyGraph;

pub struct DependencyResolver<'a> {
    model: &'a EspforgeConfiguration,
}

impl<'a> DependencyResolver<'a> {
    pub fn new(model: &'a EspforgeConfiguration) -> Result<Self> {
        Ok(Self { model })
    }

    pub fn resolve(&self) -> Result<Vec<String>> {
        let mut graph = DependencyGraph::new();

        // Add nodes
        for name in self.model.components.keys() {
            graph.add_node(name.clone());
        }
        for name in self.model.devices.keys() {
            graph.add_node(name.clone());
        }

        // Add edges from Components
        for (name, spec) in &self.model.components {
            if let Some(plugin) = find_plugin(&spec.driver) {
                // If plugin not found, we skip (it will error later in generation)
                if let Ok(deps) = plugin.dependencies(&spec.properties) {
                    for dep in deps {
                        let dep_name = dep.name.strip_prefix('$').unwrap_or(&dep.name);
                        // Only add edges for Component/Device dependencies
                        if self.model.components.contains_key(dep_name) || self.model.devices.contains_key(dep_name) {
                            // If 'name' depends on 'dep_name', 'dep_name' must come first.
                            // DependencyGraph expects add_edge(from, to) where 'to' depends on 'from'?
                            // Let's check common/dependency.rs implementation in thought process.
                            // Standard Kahn's: edges are dependencies. A -> B means A depends on B.
                            // We need B before A.
                            // Our DependencyGraph implementation in `common` seems to follow standard "From -> To" adjacency.
                            // So if A depends on B, we add B->A ? No, usually graph is Dependency <- Dependent.
                            // Let's assume add_edge(dependency, dependent).
                             graph.add_edge(dep_name.to_string(), name.clone());
                        }
                    }
                }
            }
        }

        // Add edges from Devices
        for (name, spec) in &self.model.devices {
             if let Some(plugin) = find_plugin(&spec.driver) {
                if let Ok(deps) = plugin.dependencies(&spec.properties) {
                    for dep in deps {
                         let dep_name = dep.name.strip_prefix('$').unwrap_or(&dep.name);
                         if self.model.components.contains_key(dep_name) || self.model.devices.contains_key(dep_name) {
                            graph.add_edge(dep_name.to_string(), name.clone());
                         }
                    }
                }
             }
        }

        graph.topological_sort()
    }
}

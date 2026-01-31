use std::collections::{HashMap, HashSet, VecDeque};
use anyhow::{anyhow, Result};

pub use espforge_configuration::plugin::{Dependency, DependencyKind, ResolvedDependency};

pub struct DependencyGraph {
    nodes: HashSet<String>,
    edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashSet::new(),
            edges: HashMap::new(),
        }
    }
    
    pub fn add_node(&mut self, name: String) {
        self.nodes.insert(name);
    }
    
    pub fn add_edge(&mut self, from: String, to: String) {
        self.edges.entry(from).or_insert_with(Vec::new).push(to);
    }
    
    /// Performs topological sort using Kahn's algorithm
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj_list: HashMap<String, Vec<String>> = HashMap::new();
        
        // Initialize in-degrees
        for node in &self.nodes {
            in_degree.insert(node.clone(), 0);
            adj_list.insert(node.clone(), Vec::new());
        }
        
        // Build adjacency list and calculate in-degrees
        for (from, tos) in &self.edges {
            for to in tos {
                if !self.nodes.contains(to) {
                    return Err(anyhow!("Missing dependency: {}", to));
                }
                adj_list.get_mut(from).unwrap().push(to.clone());
                *in_degree.get_mut(to).unwrap() += 1;
            }
        }
        
        // Find all nodes with in-degree 0
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(node, _)| node.clone())
            .collect();
        
        let mut result = Vec::new();
        
        while let Some(node) = queue.pop_front() {
            result.push(node.clone());
            
            if let Some(neighbors) = adj_list.get(&node) {
                for neighbor in neighbors {
                    let degree = in_degree.get_mut(neighbor).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
        
        if result.len() != self.nodes.len() {
            // Cycle detected
            let remaining: Vec<String> = self.nodes
                .iter()
                .filter(|n| !result.contains(n))
                .cloned()
                .collect();
            return Err(anyhow!("Circular dependency detected involving: {:?}", remaining));
        }
        
        Ok(result)
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}
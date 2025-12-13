use crate::config::EspforgeConfiguration;
use crate::manifest::ComponentManifest;
use anyhow::{Result, anyhow};
use inventory;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use tera::Tera;

pub mod component;
pub mod global;
pub mod logic;

/// The result of a validation check by a strategy.
#[derive(Debug)]
pub enum ValidationResult {
    /// The strategy validated the action successfully.
    Ok(String),
    /// The strategy found an error (e.g. undefined method).
    Error(String),
    /// The strategy identified the action but found a warning.
    Warning(String),
    /// The strategy does not handle this specific key pattern.
    Ignored,
}

/// Trait Definition (Design Pattern: Strategy)
pub trait ActionStrategy: Send + Sync {
    /// Returns true if this strategy claims responsibility for this action key.
    fn can_handle(&self, key: &str) -> bool;

    /// Validates the action logic (e.g. does the component exist? does the method exist?).
    /// This is used by the AppNibbler.
    fn validate(
        &self,
        key: &str,
        _value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
    ) -> ValidationResult;

    /// Renders the actual code for the action.
    /// This is used by the ContextResolver.
    fn render(
        &self,
        key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        tera: &mut Tera,
    ) -> Result<String>;
}

// --- Registration Plumbing ---

pub type ActionStrategyFactory = fn() -> Box<dyn ActionStrategy>;

pub struct ActionStrategyRegistration {
    pub factory: ActionStrategyFactory,
}

inventory::collect!(ActionStrategyRegistration);

#[macro_export]
macro_rules! register_action_strategy {
    ($strategy:ty) => {
        inventory::submit! {
            $crate::resolver::actions::ActionStrategyRegistration {
                factory: || Box::new(<$strategy>::default())
            }
        }
    };
}

// --- The Manager (Facade) ---

pub struct ActionResolver {
    strategies: Vec<Box<dyn ActionStrategy>>,
}

impl Default for ActionResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionResolver {
    pub fn new() -> Self {
        let mut strategies = Vec::new();
        for registration in inventory::iter::<ActionStrategyRegistration> {
            strategies.push((registration.factory)());
        }
        Self { strategies }
    }

    /// Finds the appropriate strategy and runs validation.
    pub fn validate(
        &self,
        key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
    ) -> ValidationResult {
        for strategy in &self.strategies {
            if strategy.can_handle(key) {
                return strategy.validate(key, value, config, manifests);
            }
        }
        ValidationResult::Warning(format!(
            "Unknown action format '{}'. No strategy found.",
            key
        ))
    }

    /// Finds the appropriate strategy and renders the code.
    pub fn resolve(
        &self,
        key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        tera: &mut Tera,
    ) -> Result<String> {
        for strategy in &self.strategies {
            if strategy.can_handle(key) {
                return strategy.render(key, value, config, manifests, tera);
            }
        }
        Err(anyhow!("No strategy found to resolve action '{}'", key))
    }
}

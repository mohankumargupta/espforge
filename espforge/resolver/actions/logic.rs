use crate::config::EspforgeConfiguration;
use crate::manifest::ComponentManifest;
use crate::register_action_strategy;
use crate::resolver::actions::{ActionResolver, ActionStrategy, ValidationResult};
use anyhow::{Result, anyhow};
use espforge_macros::auto_register_action_strategy;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use tera::Tera;

// --- Helper Functions ---

/// Resolves a YAML value into a Rust code fragment.
/// Handles:
/// - "$var" -> var (Variable reference)
/// - Strings -> "string" (Literal)
/// - Numbers/Bools -> literal
fn resolve_value(
    val: &Value,
    _config: &EspforgeConfiguration,
    _manifests: &HashMap<String, ComponentManifest>,
    _tera: &mut Tera,
) -> Result<String> {
    match val {
        Value::String(s) => {
            if let Some(var_name) = s.strip_prefix('$') {
                // It's a variable reference (e.g. "$buttonstate")
                // Return just the name so it refers to the Rust variable
                Ok(var_name.to_string())
            } else {
                // It's a string literal, wrap in quotes
                Ok(format!("\"{}\"", s))
            }
        }
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Ok(format!("{:?}", val)),
    }
}

// --- Set Strategy ---

#[derive(Default)]
#[auto_register_action_strategy]
pub struct SetActionStrategy;

impl ActionStrategy for SetActionStrategy {
    fn can_handle(&self, key: &str) -> bool {
        key == "set"
    }

    fn validate(
        &self,
        _key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        _manifests: &HashMap<String, ComponentManifest>,
    ) -> ValidationResult {
        let Some(map) = value.as_mapping() else {
            return ValidationResult::Error("'set' value must be a map".to_string());
        };

        // Check if variable exists in config
        if let Some(variable) = map.get(Value::from("variable")).and_then(|v| v.as_str()) {
            if let Some(app) = &config.app {
                if !app.variables.contains_key(variable) {
                    return ValidationResult::Error(format!(
                        "Variable '{}' is not defined in app.variables",
                        variable
                    ));
                }
            } else {
                return ValidationResult::Error("No variables defined".to_string());
            }
        } else {
            return ValidationResult::Error("'set' missing 'variable' name".to_string());
        }

        ValidationResult::Ok("Validated set action".to_string())
    }

    fn render(
        &self,
        _key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        tera: &mut Tera,
    ) -> Result<String> {
        let map = value.as_mapping().unwrap();
        let variable = map.get(Value::from("variable")).unwrap().as_str().unwrap();

        // Check for 'call' (function call assignment) OR 'value' (literal/expression)
        let resolved_value = if let Some(call_val) = map.get(Value::from("call")) {
            // Synthesize a component call action
            let call_map = call_val
                .as_mapping()
                .ok_or_else(|| anyhow!("'call' must be a map"))?;

            let target = call_map
                .get(Value::from("target"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Call missing target"))?;

            let method = call_map
                .get(Value::from("method"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Call missing method"))?;

            // Construct "$target.method" key
            let key = format!("${}.{}", target, method);

            // Use a temporary resolver to render that specific action string
            let resolver = ActionResolver::new();
            let mut code = resolver.resolve(&key, &Value::Null, config, manifests, tera)?;

            // Strip trailing semicolon if present, because we are using it as an expression
            if code.trim().ends_with(';') {
                code = code.trim().trim_end_matches(';').to_string();
            }
            code
        } else if let Some(val_node) = map.get(Value::from("value")) {
            resolve_value(val_node, config, manifests, tera)?
        } else {
            return Err(anyhow!("Set action requires either 'value' or 'call'"));
        };

        Ok(format!("{} = {};", variable, resolved_value))
    }
}

// --- If Strategy ---

#[derive(Default)]
#[auto_register_action_strategy]
pub struct IfActionStrategy;

impl ActionStrategy for IfActionStrategy {
    fn can_handle(&self, key: &str) -> bool {
        key == "if"
    }

    fn validate(
        &self,
        _key: &str,
        value: &Value,
        _config: &EspforgeConfiguration,
        _manifests: &HashMap<String, ComponentManifest>,
    ) -> ValidationResult {
        let Some(map) = value.as_mapping() else {
            return ValidationResult::Error("'if' value must be a map".to_string());
        };

        if !map.contains_key(Value::from("condition")) {
            return ValidationResult::Error("'if' missing 'condition'".to_string());
        }
        if !map.contains_key(Value::from("then")) {
            return ValidationResult::Error("'if' missing 'then' block".to_string());
        }

        ValidationResult::Ok("Validated if action".to_string())
    }

    fn render(
        &self,
        _key: &str,
        value: &Value,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        tera: &mut Tera,
    ) -> Result<String> {
        let map = value.as_mapping().unwrap();

        // condition: { lhs: ..., op: ..., rhs: ... }
        let cond_node = map
            .get(Value::from("condition"))
            .ok_or_else(|| anyhow!("Missing condition"))?;

        let cond_map = cond_node
            .as_mapping()
            .ok_or_else(|| anyhow!("Condition must be a map"))?;

        let lhs_node = cond_map.get(Value::from("lhs")).unwrap_or(&Value::Null);
        let rhs_node = cond_map.get(Value::from("rhs")).unwrap_or(&Value::Null);
        let op_node = cond_map
            .get(Value::from("op"))
            .and_then(|v| v.as_str())
            .unwrap_or("==");

        let lhs = resolve_value(lhs_node, config, manifests, tera)?;
        let rhs = resolve_value(rhs_node, config, manifests, tera)?;

        let op = match op_node {
            "equals" => "==",
            "not_equals" => "!=",
            "gt" => ">",
            "lt" => "<",
            o => o,
        };

        // Process 'then' block
        let then_node = map
            .get(Value::from("then"))
            .ok_or_else(|| anyhow!("Missing then block"))?;

        let then_list = then_node
            .as_sequence()
            .ok_or_else(|| anyhow!("'then' must be a list of actions"))?;

        let mut then_code = String::new();
        let resolver = ActionResolver::new();

        for action_val in then_list {
            if let Some(action_map) = action_val.as_mapping() {
                for (k, v) in action_map {
                    let k_str = k.as_str().unwrap_or("unknown");
                    let code = resolver.resolve(k_str, v, config, manifests, tera)?;
                    then_code.push_str(&code);
                    then_code.push('\n');
                }
            }
        }

        Ok(format!("if {} {} {} {{\n{}\n}}", lhs, op, rhs, then_code))
    }
}

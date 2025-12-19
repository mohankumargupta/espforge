use crate::config::EspforgeConfiguration;
use crate::manifest::{ComponentManifest, ParameterDef, ParameterType};
use crate::resolver::actions::ActionResolver;
use crate::resolver::strategies::{ParameterStrategy, ResolutionContext, StrategyRegistration};
use anyhow::{Context, Result, anyhow};
use inventory;
use serde::Serialize;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use tera::Tera;

pub mod actions;
pub mod ruchy_bridge;
pub mod strategies;

type ActionList = Vec<HashMap<String, Value>>;

#[derive(Debug, Serialize, Clone)]
pub struct RenderContext {
    pub includes: Vec<String>,
    pub initializations: Vec<String>,
    pub variables: Vec<String>,
    pub setup_code: Vec<String>,
    pub loop_code: Vec<String>,
    pub task_definitions: Vec<String>,
    pub task_spawns: Vec<String>,
}

pub struct ContextResolver {
    tera: Tera,
    parameter_strategies: HashMap<ParameterType, Box<dyn ParameterStrategy>>,
    action_resolver: ActionResolver,
}

impl Default for ContextResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextResolver {
    pub fn new() -> Self {
        let strategies = Self::load_registered_strategies();

        Self {
            tera: Tera::default(),
            parameter_strategies: strategies,
            action_resolver: ActionResolver::new(),
        }
    }

    fn load_registered_strategies() -> HashMap<ParameterType, Box<dyn ParameterStrategy>> {
        let mut strategies: HashMap<ParameterType, Box<dyn ParameterStrategy>> = HashMap::new();

        for registration in inventory::iter::<StrategyRegistration> {
            let (param_type, strategy) = (registration.factory)();
            strategies.insert(param_type, strategy);
        }

        strategies
    }

    pub fn resolve(
        &mut self,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
    ) -> Result<RenderContext> {
        let mut includes = Vec::new();
        let components_map = self.resolve_components(config, manifests, &mut includes)?;
        let devices_map = self.resolve_devices(config, manifests, &mut includes)?;

        let variables_code = self.resolve_variables(config)?;

        let (setup_actions, loop_actions) = self.extract_lifecycle_actions(config);

        let setup_code = self.resolve_lifecycle_block(setup_actions, "setup", config, manifests)?;
        let loop_code = self.resolve_lifecycle_block(loop_actions, "loop", config, manifests)?;

        let task_definitions = Vec::new();
        let task_spawns = Vec::new();

        // Append initializations in order (components then devices)
        let mut initializations: Vec<String> = components_map.values().cloned().collect();
        initializations.extend(devices_map.values().cloned());

        Ok(RenderContext {
            includes,
            initializations,
            variables: variables_code,
            setup_code,
            loop_code,
            task_definitions,
            task_spawns,
        })
    }

    fn resolve_variables(&self, config: &EspforgeConfiguration) -> Result<Vec<String>> {
        let mut vars = Vec::new();
        if let Some(app) = &config.app {
            for (name, var_config) in &app.variables {
                // Determine Rust type
                let rust_type = match var_config.type_name.as_str() {
                    "bool" => "bool",
                    "int" => "i32",
                    "u8" => "u8",
                    "u32" => "u32",
                    "float" => "f32",
                    _ => "i32", // default
                };

                let init_val = match &var_config.initial {
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => "0".to_string(),
                };

                vars.push(format!("let mut {} : {} = {};", name, rust_type, init_val));
            }
        }
        Ok(vars)
    }

    fn extract_lifecycle_actions<'a>(
        &self,
        config: &'a EspforgeConfiguration,
    ) -> (&'a ActionList, &'a ActionList) {
        static EMPTY_VEC: Vec<HashMap<String, Value>> = Vec::new();

        config
            .app
            .as_ref()
            .map(|app| (&app.setup, &app.loop_fn))
            .unwrap_or((&EMPTY_VEC, &EMPTY_VEC))
    }

    fn resolve_components(
        &mut self,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        includes: &mut Vec<String>,
    ) -> Result<HashMap<String, String>> {
        let Some(components) = &config.components else {
            return Ok(HashMap::new());
        };

        let resolution_ctx = ResolutionContext {
            hardware: config.esp32.as_ref(),
            platform: &config.espforge.platform,
        };

        let mut rendered_inits = HashMap::new();

        for (name, instance) in components {
            let rendered = self.resolve_single_component(
                name,
                instance,
                manifests,
                &resolution_ctx,
                includes,
            )?;
            rendered_inits.insert(name.clone(), rendered);
        }

        Ok(rendered_inits)
    }

    fn resolve_single_component(
        &mut self,
        name: &str,
        instance: &crate::config::ComponentConfig,
        manifests: &HashMap<String, ComponentManifest>,
        resolution_ctx: &ResolutionContext,
        includes: &mut Vec<String>,
    ) -> Result<String> {
        let manifest = self.get_manifest(manifests, &instance.using)?;

        let params_context = self
            .resolve_parameters(manifest, &instance.with, resolution_ctx)
            .with_context(|| format!("Failed to resolve parameters for component '{}'", name))?;

        let init_code = self.render_component_template(name, &params_context, manifest)?;

        includes.extend_from_slice(&manifest.requires);

        Ok(init_code)
    }

    fn resolve_devices(
        &mut self,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
        includes: &mut Vec<String>,
    ) -> Result<HashMap<String, String>> {
        let Some(devices) = &config.devices else {
            return Ok(HashMap::new());
        };

        let resolution_ctx = ResolutionContext {
            hardware: config.esp32.as_ref(),
            platform: &config.espforge.platform,
        };

        let mut rendered_inits = HashMap::new();

        for (name, instance) in devices {
            let rendered =
                self.resolve_single_device(name, instance, manifests, &resolution_ctx, includes)?;
            rendered_inits.insert(name.clone(), rendered);
        }

        Ok(rendered_inits)
    }

    fn resolve_single_device(
        &mut self,
        name: &str,
        instance: &crate::config::DeviceConfig,
        manifests: &HashMap<String, ComponentManifest>,
        resolution_ctx: &ResolutionContext,
        includes: &mut Vec<String>,
    ) -> Result<String> {
        let manifest = self.get_manifest(manifests, &instance.using)?;

        let params_context = self
            .resolve_parameters(manifest, &instance.with, resolution_ctx)
            .with_context(|| format!("Failed to resolve parameters for device '{}'", name))?;

        let init_code = self.render_component_template(name, &params_context, manifest)?;

        includes.extend_from_slice(&manifest.requires);

        Ok(init_code)
    }

    fn get_manifest<'a>(
        &self,
        manifests: &'a HashMap<String, ComponentManifest>,
        component_type: &str,
    ) -> Result<&'a ComponentManifest> {
        manifests
            .get(component_type)
            .ok_or_else(|| anyhow!("Component type '{}' not found in manifests", component_type))
    }

    fn render_component_template(
        &mut self,
        name: &str,
        params: &HashMap<String, Value>,
        manifest: &ComponentManifest,
    ) -> Result<String> {
        let mut render_context = tera::Context::new();
        render_context.insert("name", name);
        render_context.insert("params", params);

        self.tera
            .render_str(&manifest.setup_template, &render_context)
            .with_context(|| format!("Failed to render setup template for '{}'", name))
    }

    fn resolve_parameters(
        &self,
        manifest: &ComponentManifest,
        user_params: &HashMap<String, Value>,
        ctx: &ResolutionContext,
    ) -> Result<HashMap<String, Value>> {
        let mut resolved_params = HashMap::new();

        for param_def in &manifest.parameters {
            let value = user_params.get(&param_def.name);

            self.validate_required_parameter(param_def.required, value, &param_def.name)?;

            if let Some(val) = value {
                let resolved = self.resolve_single_parameter(param_def, val, ctx)?;
                resolved_params.insert(param_def.name.clone(), resolved);
            }
        }

        Ok(resolved_params)
    }

    fn validate_required_parameter(
        &self,
        required: bool,
        value: Option<&Value>,
        name: &str,
    ) -> Result<()> {
        if required && value.is_none() {
            return Err(anyhow!("Missing required parameter: {}", name));
        }
        Ok(())
    }

    fn resolve_single_parameter(
        &self,
        param_def: &ParameterDef,
        value: &Value,
        ctx: &ResolutionContext,
    ) -> Result<Value> {
        let strategy = self
            .parameter_strategies
            .get(&param_def.param_type)
            .ok_or_else(|| {
                anyhow!(
                    "No strategy registered for parameter type: {:?}",
                    param_def.param_type
                )
            })?;

        strategy.resolve(value, ctx)
    }

    fn resolve_lifecycle_block(
        &mut self,
        actions: &[HashMap<String, Value>],
        block_name: &str,
        config: &EspforgeConfiguration,
        manifests: &HashMap<String, ComponentManifest>,
    ) -> Result<Vec<String>> {
        actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let (key, value) = action.iter().next().ok_or_else(|| {
                    anyhow!("Empty action in {} block at index {}", block_name, index)
                })?;

                self.action_resolver
                    .resolve(key, value, config, manifests, &mut self.tera)
            })
            .collect()
    }
}

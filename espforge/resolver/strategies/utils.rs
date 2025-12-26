use std::collections::HashMap;

use crate::config::Esp32Config;
use crate::resolver::strategies::ResolutionContext;
use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_yaml_ng::Value;

pub trait ValueExt {
    fn as_ref_name(&self) -> Result<&str>;
}

impl ValueExt for Value {
    fn as_ref_name(&self) -> Result<&str> {
        let s = self.as_str()
            .ok_or_else(|| anyhow!("Reference must be a string"))?;
        
        s.strip_prefix('$')
            .ok_or_else(|| anyhow!("Reference must start with '$', got: {}", s))
    }
}

impl<'a> ResolutionContext<'a> {
    pub fn hardware(&self) -> Result<&'a Esp32Config> {
        self.hardware.ok_or_else(|| {
            anyhow!("Hardware configuration (esp32) is missing")
        })
    }
}

pub fn resolve_hardware_resource<T, V, MapFn, ViewFn>(
    value: &Value,
    ctx: &ResolutionContext,
    map_selector: MapFn,
    view_converter: ViewFn,
) -> Result<Value>
where
    MapFn: FnOnce(&Esp32Config) -> &HashMap<String, T>,
    ViewFn: FnOnce(&T) -> V,
    V: Serialize,
{
    let name = value.as_ref_name()?;
    let hardware = ctx.hardware()?;
    let map = map_selector(hardware);
    let config = map.get(name)
        .ok_or_else(|| anyhow!("Hardware resource '${}' not found", name))?;
    let view = view_converter(config);
    Ok(serde_yaml_ng::to_value(view)?)
}

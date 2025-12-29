use crate::parse::EspforgeConfiguration;
use crate::parse::processor::{ProcessorRegistration, SectionProcessor};
use anyhow::{Context, Result};
use espforge_common::Device;
use serde_yaml_ng::Value;
use std::collections::HashMap;

pub struct DeviceProvisioner;

impl SectionProcessor for DeviceProvisioner {
    fn section_key(&self) -> &'static str {
        "devices"
    }
    fn priority(&self) -> u32 {
        150 // Process after components (200)
    }

    fn process(&self, content: &Value, model: &mut EspforgeConfiguration) -> Result<()> {
        let devices: HashMap<String, Device> =
            serde_yaml_ng::from_value(content.clone()).context("Failed to deserialize devices")?;

        model.devices = devices;
        println!("✓ {} devices provisioned", model.devices.len());
        Ok(())
    }
}

inventory::submit! {
    ProcessorRegistration {
        factory: || Box::new(DeviceProvisioner),
    }
}

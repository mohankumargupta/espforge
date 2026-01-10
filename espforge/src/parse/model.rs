use crate::parse::processor::{ProcessorRegistration, SectionProcessor};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::collections::HashMap;

pub use espforge_common::{
    Component, ComponentResource, Esp32Config, GpioPinConfig, I2cConfig, PinDirection,
    ProjectModel, ResourceRef, ResourceResolver, SpiConfig, UartConfig,
};
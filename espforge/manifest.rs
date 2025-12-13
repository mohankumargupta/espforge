use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct ComponentManifest {
    pub name: String,
    pub requires: Vec<String>,
    pub parameters: Vec<ParameterDef>,
    pub setup_template: String,
    pub methods: HashMap<String, MethodDef>,
}

#[derive(Debug, Deserialize)]
pub struct ParameterDef {
    pub name: String,
    pub param_type: ParameterType,
    pub required: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Hash, Clone, Copy)]
pub enum ParameterType {
    GpioRef,
    I2cRef,
    I2cComponentRef,
    SpiRef,
    SpiComponentRef,
    UartRef,
    String,
    Integer,
    Boolean,
}

#[derive(Debug, Deserialize)]
pub struct MethodDef {
    pub template: String,
}

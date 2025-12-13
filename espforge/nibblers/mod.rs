use crate::config::EspforgeConfiguration;
use inventory;

pub trait ConfigNibbler: Send + Sync {
    fn name(&self) -> &str;

    /// Priority determines execution order.
    /// Lower numbers run first.
    fn priority(&self) -> u8 {
        50
    }

    fn process(&self, config: &EspforgeConfiguration) -> Result<NibblerResult, String>;
}

#[derive(Debug)]
pub struct NibblerResult {
    pub nibbler_name: String,
    pub findings: Vec<String>,
    pub status: NibblerStatus,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NibblerStatus {
    Ok,
    Warning,
    Error,
}

pub type NibblerFactory = fn() -> Box<dyn ConfigNibbler>;

pub struct NibblerRegistration {
    pub factory: NibblerFactory,
}

inventory::collect!(NibblerRegistration);

#[macro_export]
macro_rules! register_nibbler {
    ($nibbler:ty) => {
        inventory::submit! {
            $crate::nibblers::NibblerRegistration {
                factory: || Box::new(<$nibbler>::default())
            }
        }
    };
}

pub struct NibblerDispatcher {
    nibblers: Vec<Box<dyn ConfigNibbler>>,
}

impl Default for NibblerDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl NibblerDispatcher {
    pub fn new() -> Self {
        let mut nibblers = Vec::new();

        for registration in inventory::iter::<NibblerRegistration> {
            nibblers.push((registration.factory)());
        }

        // Sort by priority, lower number higher priority
        nibblers.sort_by_key(|a| a.priority());

        Self { nibblers }
    }

    pub fn process_config(&self, config: &EspforgeConfiguration) -> Vec<NibblerResult> {
        self.nibblers
            .iter()
            .filter_map(|nibbler| match nibbler.process(config) {
                Ok(result) => Some(result),
                Err(e) => {
                    eprintln!("Internal Error in nibbler '{}': {}", nibbler.name(), e);
                    None
                }
            })
            .collect()
    }
}

pub mod app;
pub mod components;
pub mod esp32;
pub mod project;
pub mod template;

pub mod logger;
pub mod components;
pub mod gpio; 

pub struct Context {
    pub logger: logger::Logger,
}

impl Context {
    pub fn new() -> Self {
        Self {
            logger: logger::Logger::new(),
        }
    }
}

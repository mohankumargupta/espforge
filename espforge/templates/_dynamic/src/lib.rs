#![no_std]

pub mod components;
pub mod globals;
pub mod platform;
pub mod devices; 

pub mod prelude {
    pub use crate::components::*;
    pub use crate::devices::*;
    pub use crate::globals::*;
}


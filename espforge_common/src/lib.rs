// espforge_common/src/lib.rs
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
pub type ConfigString = String;

#[cfg(not(feature = "std"))]
pub type ConfigString = &'static str;

pub mod components;

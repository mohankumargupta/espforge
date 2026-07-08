//! espforge-runtime: `no_std` runtime implementations of each capability.
//!
//! Unlike esphome, this crate keeps **separate `components` and `devices`
//! modules** (ADR-007), mirroring the three-tier domain spine (ADR-003).

#![no_std]

pub mod components;
pub mod devices;

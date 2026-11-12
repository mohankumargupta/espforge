#![no_std]
pub mod generated;
pub use espforge_runtime::Delay;
pub use espforge_runtime::embedded_hal;
/// Access a device from the `Context`. Usage: `device!(ctx, my_screen)`
#[macro_export]
macro_rules! device {
    ($ctx:expr, $name:ident) => {
        & $ctx .devices. $name
    };
}
/// Backward compatibility alias for `device!`
#[macro_export]
macro_rules! component {
    ($ctx:expr, $name:ident) => {
        $crate::device!($ctx, $name)
    };
}
/// Access a service from the `Context`. Usage: `service!(ctx, wifi)`
#[macro_export]
macro_rules! service {
    ($ctx:expr, $name:ident) => {
        & $ctx .services. $name
    };
}
/// Define a cross-task signal. Usage: `crate::signal!(BUTTON_PRESSED)`
#[macro_export]
macro_rules! signal {
    ($name:ident) => {
        static $name : embassy_sync::signal::Signal <
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, (), > =
        embassy_sync::signal::Signal::new();
    };
}
pub mod app;
pub use generated::{Devices, PeripheralRegistry, Services};
/// The whole context the app receives.
pub struct Context {
    pub logger: espforge_runtime::Logger,
    pub delay: espforge_runtime::Delay,
    pub devices: Devices,
    pub services: Services,
}
/// Set by the entry point once wiring is complete.
pub static CTX: static_cell::StaticCell<Context> = static_cell::StaticCell::new();

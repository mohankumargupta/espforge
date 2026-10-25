//! `# services` lowering: one module per service.
//!
//! Only `<wifi>` is buildable today; anything else is a clear "not
//! supported yet" from [`wifi::lower_services`].

pub mod wifi;

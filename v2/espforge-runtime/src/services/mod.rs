//! Software-service capabilities (ADR-013). Unlike `components/`, these do not
//! claim peripherals; they consume the implicit network `Stack` (ADR-012). Each
//! module is gated by its `espforge-runtime` feature and re-exported into
//! `crate::components` for a uniform `ctx.components.http` accessor.

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "http")]
pub use http::Http;

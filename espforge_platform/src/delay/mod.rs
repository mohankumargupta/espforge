//! Unified delay abstraction for both blocking and async modes
//! 
//! # Usage
//! 
//! ## Blocking mode
//! ```rust,ignore
//! pub fn forever(ctx: &mut Context) {
//!     ctx.delay.delay_ms(1000);
//! }
//! ```
//! 
//! ## Async mode (Embassy)
//! ```rust,ignore
//! pub async fn forever(ctx: &mut Context<'_>) {
//!     ctx.delay.delay_ms(1000).await;
//! }
//! ```

#[cfg(not(feature = "embassy"))]
mod blocking;
#[cfg(not(feature = "embassy"))]
pub use blocking::Delay;

#[cfg(feature = "embassy")]
mod embassy;
#[cfg(feature = "embassy")]
pub use embassy::Delay;
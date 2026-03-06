#[cfg(feature = "wifi")]
mod wrapper;

#[cfg(feature = "wifi")]
pub use wrapper::{WifiError, WifiResources};

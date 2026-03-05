#[cfg(feature = "wifi")]
mod wrapper;

#[cfg(feature = "wifi")]
pub use wrapper::{HttpResponse, WifiClient, WifiError, WifiResources};

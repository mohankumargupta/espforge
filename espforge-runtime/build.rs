//! Record the firmware build time so the NTP era hint is decided when the
//! firmware is built, not when the project is generated.
//!
//! `SOURCE_DATE_EPOCH` wins when it is set, which keeps CI and reproducible
//! builds deterministic; otherwise the wall clock is used. The value never
//! reaches `generated.rs`, so goldens stay byte-identical.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let build_unix = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.parse::<u64>().ok())
        .or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|elapsed| elapsed.as_secs())
        })
        .unwrap_or(0);

    println!("cargo:rustc-env=ESPFORGE_BUILD_UNIX={build_unix}");
}

#![allow(unexpected_cfgs)]
#![cfg(feature = "async")]

use embassy_sync::signal::Signal;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

pub static GLOBAL_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub struct SignalBridge;

#[allow(non_upper_case_globals)]
pub const signal: SignalBridge = SignalBridge;

impl SignalBridge {
    pub fn signal(&self) {
        GLOBAL_SIGNAL.signal(());
    }

    // Forward the wait call
    pub async fn wait(&self) -> () {
        GLOBAL_SIGNAL.wait().await
    }

    pub fn try_take(&self) -> Option<()> {
        GLOBAL_SIGNAL.try_take()
    }

    pub fn reset(&self) {
        GLOBAL_SIGNAL.reset();
    }

    pub fn signaled(&self) -> bool {
        GLOBAL_SIGNAL.signaled()
    }
}


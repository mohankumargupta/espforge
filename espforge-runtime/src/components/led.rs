//! `led` component: a single GPIO output, wired move-by-value (ADR-008).
//!
//! The `Output` is held behind a `RefCell` so the mutating methods take
//! `&self` (mirroring `Button`). This keeps the blocking `component!(ctx,
//! led).toggle()` call site working (`&mut` is still accepted) while also
//! letting an embassy task borrow the LED as a shared `&'static Context` and
//! toggle it from a spawned task (see the multi-LED blink example).

use core::cell::RefCell;
use esp_hal::gpio::{Level, Output, OutputConfig, OutputPin};

pub struct Led {
    pin: RefCell<Output<'static>>,
    active_low: bool,
}

impl Led {
    /// `pin` is the output pin moved in by value. Starting `off` depends on
    /// `active_low`: an active-low LED is off while the pin reads high.
    pub fn new(pin: impl OutputPin + 'static, active_low: bool) -> Self {
        let off = if active_low { Level::High } else { Level::Low };
        Led {
            pin: RefCell::new(Output::new(pin, off, OutputConfig::default())),
            active_low,
        }
    }

    pub fn on(&self) {
        let level = if self.active_low {
            Level::Low
        } else {
            Level::High
        };
        self.pin.borrow_mut().set_level(level);
    }

    pub fn off(&self) {
        let level = if self.active_low {
            Level::High
        } else {
            Level::Low
        };
        self.pin.borrow_mut().set_level(level);
    }

    pub fn toggle(&self) {
        self.pin.borrow_mut().toggle();
    }
}

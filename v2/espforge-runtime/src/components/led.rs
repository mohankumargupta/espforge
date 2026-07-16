//! `led` component: a single GPIO output, wired move-by-value (ADR-008).

use core::cell::RefCell;
use esp_hal::gpio::{Level, Output, OutputConfig};

/// A single GPIO output LED.
///
/// The underlying `Output` is held behind a `RefCell` guarded by a
/// `critical_section` so the mutating methods (`on`/`off`/`toggle`) take
/// `&self`. This keeps the blocking `component!(ctx, led).toggle()` call site
/// working (`&mut` is still accepted) while also letting an embassy task borrow
/// the LED as a shared `&'static` and toggle it from a spawned task (see the
/// multi-LED blink example).
pub struct Led {
    pin: RefCell<Output<'static>>,
    active_low: bool,
}

impl Led {
    /// `pin` is the output pin moved in by value. `active_low` selects polarity.
    pub fn new(pin: Output<'static>, active_low: bool) -> Self {
        Led {
            pin: RefCell::new(pin),
            active_low,
        }
    }

    pub fn on(&self) {
        let level = if self.active_low { Level::High } else { Level::Low };
        critical_section::with(|_cs| self.pin.borrow_mut().set_level(level));
    }

    pub fn off(&self) {
        let level = if self.active_low { Level::Low } else { Level::High };
        critical_section::with(|_cs| self.pin.borrow_mut().set_level(level));
    }

    pub fn toggle(&self) {
        critical_section::with(|_cs| self.pin.borrow_mut().toggle());
    }
}

/// Build an `Output` from a moved-in GPIO peripheral (used by the codegen).
pub fn output(pin: impl esp_hal::gpio::OutputPin + 'static, active_low: bool) -> Led {
    let level = if active_low { Level::High } else { Level::Low };
    Led::new(Output::new(pin, level, OutputConfig::default()), active_low)
}

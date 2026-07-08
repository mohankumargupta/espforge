//! `led` component: a single GPIO output, wired move-by-value (ADR-008).

use esp_hal::gpio::Output;

pub struct Led {
    pin: Output<'static>,
    active_low: bool,
}

impl Led {
    /// `pin` is the output pin moved in by value. `active_low` selects polarity.
    pub fn new(pin: Output<'static>, active_low: bool) -> Self {
        Led { pin, active_low }
    }

    pub fn on(&mut self) {
        if self.active_low {
            self.pin.set_low();
        } else {
            self.pin.set_high();
        }
    }

    pub fn off(&mut self) {
        if self.active_low {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
    }

    pub fn toggle(&mut self) {
        self.pin.toggle();
    }
}

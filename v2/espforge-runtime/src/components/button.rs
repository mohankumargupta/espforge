//! `button` component: a single GPIO input with an optional pull-up, wired
//! move-by-value (ADR-008).

use esp_hal::gpio::{Input, InputConfig, Pull};

pub struct Button {
    pin: Input<'static>,
    pull_up: bool,
}

impl Button {
    /// `pin` is the input pin moved in by value. `pull_up` records the configured
    /// pull resistor (informational; the pull is set by the codegen).
    pub fn new(pin: Input<'static>, pull_up: bool) -> Self {
        Button { pin, pull_up }
    }

    pub fn is_pressed(&mut self) -> bool {
        // pulled-up buttons read low when pressed.
        if self.pull_up {
            self.pin.is_low()
        } else {
            self.pin.is_high()
        }
    }
}

/// Build an `Input` from a moved-in GPIO peripheral (used by the codegen).
pub fn input(pin: impl esp_hal::gpio::InputPin + 'static, pull_up: bool) -> Button {
    let pull = if pull_up { Pull::Up } else { Pull::None };
    Button::new(
        Input::new(pin, InputConfig::default().with_pull(pull)),
        pull_up,
    )
}

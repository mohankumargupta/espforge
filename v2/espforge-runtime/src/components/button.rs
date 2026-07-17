//! `button` component: a single GPIO input with an optional pull-up, wired
//! move-by-value (ADR-008).
//!
//! The `Input` is held behind a `RefCell` so the mutating/awaiting methods
//! take `&self` (mirroring `Led`). This lets an embassy task hold a shared
//! `&'static Context` and call `wait_for_pressed().await` (and `is_pressed()`)
//! without needing a `&mut` borrow of the context — the same reason `Led`
//! uses interior mutability so a spawned task can `toggle(&self)`.

use core::cell::RefCell;
use esp_hal::gpio::{Input, InputConfig, Pull};

pub struct Button {
    pin: RefCell<Input<'static>>,
    pull_up: bool,
}

impl Button {
    /// `pin` is the input pin moved in by value. `pull_up` records the configured
    /// pull resistor (informational; the pull is set by the codegen).
    pub fn new(pin: Input<'static>, pull_up: bool) -> Self {
        Button {
            pin: RefCell::new(pin),
            pull_up,
        }
    }

    /// Current pressed state. Pulled-up buttons read low when pressed.
    pub fn is_pressed(&self) -> bool {
        // pulled-up buttons read low when pressed.
        if self.pull_up {
            self.pin.borrow().is_low()
        } else {
            self.pin.borrow().is_high()
        }
    }

    /// Asynchronously wait until the button is pressed (edge-triggered, not
    /// polled). Pulled-up buttons read low when pressed, so we wait for low;
    /// otherwise we wait for high. Requires the `embassy` feature (the
    /// `esp-hal` async `Input` wait methods).
    #[cfg(feature = "embassy")]
    pub async fn wait_for_pressed(&self) {
        if self.pull_up {
            self.pin.borrow_mut().wait_for_low().await;
        } else {
            self.pin.borrow_mut().wait_for_high().await;
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

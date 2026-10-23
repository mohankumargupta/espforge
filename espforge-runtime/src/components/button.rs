//! `button` component: a single GPIO input with an optional pull-up, wired
//! move-by-value (ADR-008).
//!
//! The `Input` is held behind a `RefCell` so the mutating/awaiting methods
//! take `&self` (mirroring `Led`). This lets an embassy task hold a shared
//! `&'static Context` and call `wait_for_pressed().await` (and `is_pressed()`)
//! without needing a `&mut` borrow of the context — the same reason `Led`
//! uses interior mutability so a spawned task can `toggle(&self)`.

use core::cell::RefCell;
use esp_hal::gpio::{Input, InputConfig, InputPin, Pull};

pub struct Button {
    pin: RefCell<Input<'static>>,
    pull_up: bool,
}

impl Button {
    /// `pin` is the input pin moved in by value. `pull_up` selects the
    /// internal pull resistor: a pulled-up button reads low when pressed
    /// (the pin wired to ground through the switch).
    pub fn new(pin: impl InputPin + 'static, pull_up: bool) -> Self {
        let pull = if pull_up { Pull::Up } else { Pull::None };
        Button {
            pin: RefCell::new(Input::new(pin, InputConfig::default().with_pull(pull))),
            pull_up,
        }
    }

    /// Current pressed state. Pulled-up buttons read low when pressed.
    pub fn is_pressed(&self) -> bool {
        if self.pull_up {
            self.pin.borrow().is_low()
        } else {
            self.pin.borrow().is_high()
        }
    }

    /// Asynchronously wait until the button is pressed (edge-triggered, not
    /// polled). Pulled-up buttons read low when pressed, so we wait for low;
    /// otherwise we wait for high. Requires the `embassy` feature so the
    /// shared context can be borrowed by a spawned task while waiting.
    #[cfg(feature = "embassy")]
    pub async fn wait_for_pressed(&self) {
        if self.pull_up {
            self.pin.borrow_mut().wait_for_low().await;
        } else {
            self.pin.borrow_mut().wait_for_high().await;
        }
    }
}

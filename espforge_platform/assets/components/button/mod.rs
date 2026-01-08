use crate::platform::gpio::GPIOInput;
use embedded_hal::digital::InputPin;

pub struct Button {
    input: GPIOInput,
}

impl Button {
    pub fn new(input: GPIOInput) -> Self {
        Button {
            input,
        }
    }

    pub fn is_button_pressed(&mut self) -> bool {
        // Assuming active low logic for the button
        self.input.is_low().unwrap_or(false)
    }
}
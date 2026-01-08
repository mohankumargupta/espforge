use crate::platform::gpio::GPIOInput;
use embedded_hal::digital::InputPin;

pub struct Button {
    input: GPIOInput,
}

impl Button {
    pub fn new(pin: u8,  pullup: bool, pulldown: bool) -> Self {
        Button {
            input: GPIOInput::new(pin, pullup, pulldown),
        }
    }

    pub fn is_button_pressed(&mut self) -> bool {
        // Assuming active low logic for the button
        self.input.is_low().unwrap_or(false)
    }
}

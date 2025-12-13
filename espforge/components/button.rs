
use crate::platform::gpio::GPIOInput;

pub struct Button {
    input: GPIOInput,
}

impl Button {
    pub fn new(pin: u8,  pullup: bool, pulldown: bool) -> Self {
        let (final_up, final_down) = if !pullup && !pulldown {
            (true, false)
        } else {
            (pullup, pulldown)
        };

        Button {
            input: GPIOInput::new(pin, final_up, final_down),
        }
    }


    pub fn is_button_pressed(&self) -> bool {
        self.input.is_low()
    }
}

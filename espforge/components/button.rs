
use crate::platform::gpio::GPIOInput;

pub struct Button {
    input: GPIOInput,
}

impl Button {
    pub fn new(pin: u8,  pullup: bool, pulldown: bool) -> Self {
        Button {
            input: GPIOInput::new(pin, pullup, pulldown),
        }
    }


    pub fn is_button_pressed(&self) -> bool {
        self.input.is_low()
    }

    pub async fn wait_for_press(&mut self) {
        self.input.wait_for_falling_edge().await;
    }
    
    pub async fn wait_for_release(&mut self) {
        self.input.wait_for_rising_edge().await;
    }
}

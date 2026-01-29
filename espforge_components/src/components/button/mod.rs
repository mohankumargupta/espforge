use espforge_platform::gpio::GPIOInput;
use embedded_hal::digital::InputPin;
pub use espforge_common::components::button::ButtonConfig;

pub struct Button {
    input: GPIOInput,
    config: ButtonConfig,
}

impl Button {
    pub fn new(input: GPIOInput, config: ButtonConfig) -> Self {
        Button {
            input,
            config,
        }
    }

    pub fn is_button_pressed(&mut self) -> bool {
        if self.config.pull_up {
            self.input.is_low().unwrap_or(false)
        } else {
            self.input.is_high().unwrap_or(false)
        }
    }
}
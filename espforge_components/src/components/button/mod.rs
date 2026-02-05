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

#[cfg(feature = "embassy")]
impl Button {
    /// Wait for the button to be pressed (async)
    pub async fn wait_for_pressed(&mut self) {
        if self.config.pull_up {
            self.input.wait_for_falling_edge().await;
        } else {
            self.input.wait_for_rising_edge().await;
        }
    }

    /// Wait for the button to be released (async)
    pub async fn wait_for_released(&mut self) {
        if self.config.pull_up {
            self.input.wait_for_rising_edge().await;
        } else {
            self.input.wait_for_falling_edge().await;
        }
    }

    /// Wait for any edge (press or release)
    pub async fn wait_for_any_edge(&mut self) {
        self.input.wait_for_any_edge().await;
    }
}

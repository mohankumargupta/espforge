use embedded_hal::digital::{OutputPin, StatefulOutputPin};
pub use espforge_common::components::led::LedConfig;
use espforge_platform::gpio::GPIOOutput;

pub struct LED {
    output: GPIOOutput,
    config: LedConfig,
}

impl LED {
    pub fn new(output: GPIOOutput, config: LedConfig) -> Self {
        let mut led = Self { output, config };
        led.off();
        led
    }

    pub fn on(&mut self) {
        if self.config.active_low {
            self.output.set_low().expect("Failed to turn LED on");
        } else {
            self.output.set_high().expect("Failed to turn LED on");
        }
    }

    pub fn off(&mut self) {
        if self.config.active_low {
            self.output.set_high().expect("Failed to turn LED off");
        } else {
            self.output.set_low().expect("Failed to turn LED off");
        }
    }

    pub fn toggle(&mut self) {
        self.output.toggle().expect("Failed to toggle LED");
    }
}

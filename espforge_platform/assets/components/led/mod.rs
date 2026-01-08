use crate::platform::gpio::GPIOOutput;
use embedded_hal::digital::{OutputPin, StatefulOutputPin};

pub struct LED {
    output: GPIOOutput,
}

impl LED {
    pub fn new(output: GPIOOutput) -> Self {
        LED {
            output
        }
    }

    pub fn on(&mut self) {
        self.output.set_high().expect("Failed to turn LED on");
    }

    pub fn off(&mut self) {
        self.output.set_low().expect("Failed to turn LED off");
    }

    pub fn toggle(&mut self) {
        self.output.toggle().expect("Failed to toggle LED");
    }
}

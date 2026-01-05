
use crate::platform::gpio::GPIOOutput;

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
        self.output.set_high();
    }

    pub fn off(&mut self) {
        self.output.set_low();
    }

    pub fn toggle(&mut self) {
        self.output.toggle();
    }
}

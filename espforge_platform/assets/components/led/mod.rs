
use crate::platform::gpio::GPIOOutput;

pub mod delay;

pub struct LED {
    output: GPIOOutput,
}

impl LED {
    pub fn new(pin: u8) -> Self {
        LED {
            output: GPIOOutput::new(pin),
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

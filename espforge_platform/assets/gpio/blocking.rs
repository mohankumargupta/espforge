use esp_hal::{
    gpio::{AnyPin, Input, InputConfig, Level, Output, OutputConfig, Pull},
};
use embedded_hal::digital::{ErrorType, InputPin, OutputPin, StatefulOutputPin};
use core::convert::Infallible;

pub struct GPIOOutput {
    output: Output<'static>,
}

impl GPIOOutput {
    pub fn from_pin(pin: AnyPin<'static>) -> Self {
        let config = OutputConfig::default();
        let output = Output::new(pin, Level::Low, config);
        Self { output }
    }

    pub fn new(pin_number: u8) -> Self {
        let any_pin = unsafe { AnyPin::steal(pin_number) };
        Self::from_pin(any_pin)
    }
}


impl ErrorType for GPIOOutput {
    type Error = Infallible;
}

impl OutputPin for GPIOOutput {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.output.set_low();
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.output.set_high();
        Ok(())
    }
}

impl StatefulOutputPin for GPIOOutput {
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.output.is_set_high())
    }

    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.output.is_set_low())
    }

    fn toggle(&mut self) -> Result<(), Self::Error> {
        self.output.toggle();
        Ok(())
    }
}

pub struct GPIOInput {
    input: Input<'static>,
}

impl GPIOInput {
    pub fn from_pin(pin: AnyPin<'static>, pull_up: bool, pull_down: bool) -> Self {
        let pull = if pull_up { Pull::Up } else if pull_down { Pull::Down } else { Pull::None };
        let config = InputConfig::default().with_pull(pull);
        let input = Input::new(pin, config);
        Self { input }
    }

    pub fn new(pin_number: u8, pull_up: bool, pull_down: bool) -> Self {
        let any_pin = unsafe { AnyPin::steal(pin_number) };
        Self::from_pin(any_pin, pull_up, pull_down)
    }
}


impl ErrorType for GPIOInput {
    type Error = Infallible;
}

impl InputPin for GPIOInput {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input.is_high())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input.is_low())
    }
}

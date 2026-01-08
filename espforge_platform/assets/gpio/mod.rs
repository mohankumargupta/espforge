use esp_hal::{
    gpio::{AnyPin, Input, InputConfig, Level, Output, OutputConfig, Pull},
};
use embedded_hal::digital::{ErrorType, InputPin, OutputPin, StatefulOutputPin};

/// User-friendly GPIO output wrapper implementing embedded-hal traits
pub struct GPIOOutput {
    output: Output<'static>,
}

impl GPIOOutput {
    /// Creates a wrapper from an existing owned pin (Registry Pattern)
    pub fn from_pin(pin: AnyPin<'static>) -> Self {
        let config = OutputConfig::default();
        let output = Output::new(pin, Level::Low, config);
        Self { output }
    }

    /// Legacy constructor (Integer Pattern)
    pub fn new(pin_number: u8) -> Self {
        let any_pin = unsafe { AnyPin::steal(pin_number) };
        Self::from_pin(any_pin)
    }
}

impl ErrorType for GPIOOutput {
    type Error = core::convert::Infallible;
}

impl OutputPin for GPIOOutput {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.output.set_low())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.output.set_high())
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
        Ok(self.output.toggle())
    }
}

/// User-friendly GPIO input wrapper implementing embedded-hal traits
pub struct GPIOInput {
    input: Input<'static>,
}

impl GPIOInput {
    /// Creates a wrapper from an existing owned pin (Registry Pattern)
    pub fn from_pin(pin: AnyPin<'static>, pull_up: bool, pull_down: bool) -> Self {
        let pull = if pull_up { Pull::Up } else if pull_down { Pull::Down } else { Pull::None };
        let config = InputConfig::default().with_pull(pull);
        let input = Input::new(pin, config);
        Self { input }
    }

    /// Legacy constructor
    pub fn new(pin_number: u8, pull_up: bool, pull_down: bool) -> Self {
        let any_pin = unsafe { AnyPin::steal(pin_number) };
        Self::from_pin(any_pin, pull_up, pull_down)
    }
}

impl ErrorType for GPIOInput {
    type Error = core::convert::Infallible;
}

impl InputPin for GPIOInput {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input.is_high())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input.is_low())
    }
}

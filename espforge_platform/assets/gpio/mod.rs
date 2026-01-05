use esp_hal::{
    gpio::{AnyPin, Input, InputConfig, Level, Output, OutputConfig, Pull},
};

/// User-friendly GPIO output wrapper
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

    pub fn set_high(&mut self) { self.output.set_high(); }
    pub fn set_low(&mut self) { self.output.set_low(); }
    pub fn toggle(&mut self) { self.output.toggle(); }
    pub fn is_high(&self) -> bool { self.output.is_set_high() }
}

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

    pub fn read(&self) -> Level { self.input.level() }
    pub fn is_high(&self) -> bool { self.input.is_high() }
    pub fn is_low(&self) -> bool { self.input.is_low() }
    // pub async fn wait_for_low(&mut self) { self.input.wait_for_low().await; }
    // pub async fn wait_for_high(&mut self) { self.input.wait_for_high().await; }
    // pub async fn wait_for_rising_edge(&mut self) { self.input.wait_for_rising_edge().await; }
    // pub async fn wait_for_falling_edge(&mut self) { self.input.wait_for_falling_edge().await; }
}


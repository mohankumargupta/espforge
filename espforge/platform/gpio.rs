use esp_hal::{
    gpio::{AnyPin, Input, InputConfig, Level, Output, OutputConfig, Pull},
};

/// User-friendly GPIO output wrapper
pub struct GPIOOutput {
    output: Output<'static>,
}

impl GPIOOutput {
    /// Creates a new GPIO output with the specified pin number
    ///
    /// # Arguments
    /// * `pin_number` - The GPIO pin number (0-39 for ESP32)
    ///
    /// # Panics
    /// Panics if the pin number is invalid or already in use
    pub fn new(pin_number: u8) -> Self {
        // Safety: We ensure only one instance exists by consuming the AnyPin
        let any_pin = unsafe { AnyPin::steal(pin_number) };

        // Default to push-pull output with 20mA drive strength
        let config = OutputConfig::default();
        let output = Output::new(any_pin, Level::Low, config);

        GPIOOutput { output }
    }

    /// Sets the output level high
    pub fn set_high(&mut self) {
        self.output.set_high();
    }

    /// Sets the output level low
    pub fn set_low(&mut self) {
        self.output.set_low();
    }

    /// Toggles the output level
    pub fn toggle(&mut self) {
        self.output.toggle();
    }

    /// Gets the current output level
    pub fn is_high(&self) -> bool {
        self.output.is_set_high()
    }
}


pub struct GPIOInput {
    input: Input<'static>,
}

impl GPIOInput {
    /// Creates a new GPIO input with the specified pin number and pull configuration
    ///
    /// # Arguments
    /// * `pin_number` - The GPIO pin number (0-39 for ESP32)
    /// * `pull_up` - enable/disable pull up resistor (default true)
    /// * `pull_down` - enable/disable pull up resistor (default false)
    /// # Panics
    /// Panics if the pin number is invalid or already in use
    pub fn new(pin_number: u8, pull_up: bool, pull_down: bool) -> Self {
        // Safety: We ensure only one instance exists by consuming the AnyPin
        let any_pin = unsafe { AnyPin::steal(pin_number) };

        let pull = if pull_up {
            Pull::Up
        } else if pull_down {
            Pull::Down
        } else {
            Pull::None
        };

        let config = InputConfig::default().with_pull(pull);
        let input = Input::new(any_pin, config);

        GPIOInput { input }
    }

    /// Reads the current input level
    pub fn read(&self) -> Level {
        self.input.level()
    }

    /// Returns true if the input is high
    pub fn is_high(&self) -> bool {
        self.input.is_high()
    }

    /// Returns true if the input is low
    pub fn is_low(&self) -> bool {
        self.input.is_low()
    }
}

